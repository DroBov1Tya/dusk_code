//! Asking the user before a tool leaves its folders or runs a command. The call waits for
//! the answer, shown by the client; full access answers yes at once. A yes for the
//! session lasts until the core stops and is never written down.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;

use anyhow::{Result, bail};
use tokio::sync::{mpsc, oneshot};

use super::tool::Progress;
use crate::{config::ApprovalPolicy, runtime::ThreadEvent};

pub use dusk_protocol::tools::{ApprovalRequest, Request, WindowAccess};
use dusk_protocol::tools::{folder_of, program};

pub use dusk_protocol::tools::Decision;

/// What decides a request in the user's place, in arbiter mode.
pub trait Judge: Send + Sync {
    /// Never an error: a judge that cannot decide says no.
    fn judge<'a>(&'a self, request: &'a Request) -> crate::BoxFuture<'a, Verdict>;
}

pub struct Verdict {
    pub allow: bool,
    pub reason: String,
}

#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Decision>>>,
    grants: Mutex<Grants>,
    judge: std::sync::OnceLock<std::sync::Arc<dyn Judge>>,
}

impl Approvals {
    pub fn set_judge(&self, judge: std::sync::Arc<dyn Judge>) {
        let _ = self.judge.set(judge);
    }

    /// Waits for the user's answer; an error unless it is yes. Without `progress` no
    /// client listens, so only full access or a session grant lets the request through.
    pub async fn ask(
        &self,
        request: Request,
        progress: Option<Progress>,
        policy: ApprovalPolicy,
    ) -> Result<()> {
        // In sandbox mode a command needs no yes: the system confines it when it runs.
        let confined =
            policy == ApprovalPolicy::Sandbox && matches!(request, Request::Command { .. });
        if policy == ApprovalPolicy::FullAccess || confined || self.grants.lock().covers(&request) {
            return Ok(());
        }
        if policy == ApprovalPolicy::Arbiter {
            return self.arbitrate(request, progress).await;
        }
        let Some(progress) = progress else {
            bail!(
                "{} needs the user's approval, and no one can answer here",
                request.what()
            );
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, answer) = oneshot::channel();
        self.pending.lock().insert(id, sender);
        // Closes the request on the client however the wait ends, a stopped turn included.
        let _open = Open {
            approvals: self,
            id,
            events: progress.events.clone(),
        };
        progress.emit(ThreadEvent::Approval(ApprovalRequest {
            id,
            request: request.clone(),
        }));
        match answer.await {
            Ok(Decision::Once) => Ok(()),
            Ok(Decision::Session) => {
                self.grants.lock().add(&request);
                Ok(())
            }
            Ok(Decision::Deny) | Err(_) => bail!(
                "the user denied {}; do not try it another way: ask them, or go on without it",
                request.what()
            ),
        }
    }

    /// The arbiter's verdict stands for the user's answer; the thread that asked hears it.
    async fn arbitrate(&self, request: Request, progress: Option<Progress>) -> Result<()> {
        let what = request.what();
        let verdict = match self.judge.get() {
            Some(judge) => judge.judge(&request).await.unwrap_or_else(|error| Verdict {
                allow: false,
                reason: format!("the arbiter could not decide: {error:#}"),
            }),
            None => Verdict {
                allow: false,
                reason: "no arbiter is running".into(),
            },
        };
        if let Some(progress) = progress {
            progress.emit(ThreadEvent::ArbiterDecided {
                what: what.clone(),
                allow: verdict.allow,
                reason: verdict.reason.clone(),
            });
        }
        match verdict.allow {
            true => Ok(()),
            false => bail!(
                "the arbiter denied {what}: {}; do not try it another way: go on without \
                 it or ask the user",
                verdict.reason
            ),
        }
    }

    /// False when nothing waits for `id`, such as after its turn was stopped.
    pub fn answer(&self, id: u64, decision: Decision) -> bool {
        self.pending
            .lock()
            .remove(&id)
            .is_some_and(|sender| sender.send(decision).is_ok())
    }
}

struct Open<'a> {
    approvals: &'a Approvals,
    id: u64,
    events: mpsc::UnboundedSender<ThreadEvent>,
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.approvals.pending.lock().remove(&self.id);
        let _ = self
            .events
            .send(ThreadEvent::ApprovalClosed { id: self.id });
    }
}

/// What the user allowed for the session.
#[derive(Default)]
struct Grants {
    /// Folders with whether writing is allowed too.
    folders: Vec<(PathBuf, bool)>,
    programs: Vec<String>,
    screen: bool,
    system_pointer: bool,
    windows: Vec<WindowAccess>,
}

impl Grants {
    fn covers(&self, request: &Request) -> bool {
        match request {
            Request::Path { path, write } => self
                .folders
                .iter()
                .any(|(folder, writable)| path.starts_with(folder) && (*writable || !write)),
            Request::Command { command } => {
                program(command).is_some_and(|name| self.programs.iter().any(|p| p == name))
            }
            Request::Screen { .. } => self.screen,
            Request::SystemPointer { .. } => self.system_pointer,
            Request::Keyboard { .. } => false,
            Request::Window { grant, .. } => grant
                .as_ref()
                .is_some_and(|access| self.windows.contains(access)),
        }
    }

    fn add(&mut self, request: &Request) {
        match request {
            Request::Path { path, write } => self.folders.push((folder_of(path), *write)),
            Request::Command { command } => {
                self.programs.extend(program(command).map(str::to_owned))
            }
            Request::Screen { .. } => self.screen = true,
            Request::SystemPointer { .. } => self.system_pointer = true,
            Request::Keyboard { .. } => {}
            Request::Window { grant, .. } => self.windows.extend(grant.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn decide(approvals: &Approvals, request: Request, decision: Decision) -> Result<()> {
        let (events, mut seen) = mpsc::unbounded_channel();
        let ask = approvals.ask(
            request,
            Some(Progress {
                call_id: "pointer-test".into(),
                events,
            }),
            ApprovalPolicy::Ask,
        );
        let answer = async {
            let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
                panic!("missing request");
            };
            assert!(approvals.answer(asked.id, decision));
        };
        let (result, ()) = tokio::join!(ask, answer);
        result
    }

    #[tokio::test]
    async fn screen_session_never_authorizes_system_pointer_control() {
        let approvals = Approvals::default();
        let screen = Request::Screen {
            action: "activate without moving the pointer".into(),
        };
        let pointer = Request::SystemPointer {
            action: "move the pointer and click".into(),
        };
        decide(&approvals, screen.clone(), Decision::Session)
            .await
            .unwrap();
        assert!(
            approvals
                .ask(screen, None, ApprovalPolicy::Ask)
                .await
                .is_ok()
        );
        for policy in [
            ApprovalPolicy::Ask,
            ApprovalPolicy::Sandbox,
            ApprovalPolicy::Arbiter,
        ] {
            assert!(
                approvals.ask(pointer.clone(), None, policy).await.is_err(),
                "{policy:?}"
            );
        }
        assert!(
            decide(&approvals, pointer.clone(), Decision::Deny)
                .await
                .is_err()
        );
        assert!(
            approvals
                .ask(pointer.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        decide(&approvals, pointer.clone(), Decision::Once)
            .await
            .unwrap();
        assert!(
            approvals
                .ask(pointer.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        // FullAccess is an explicit global policy, not an inferred screen grant.
        assert!(
            approvals
                .ask(pointer.clone(), None, ApprovalPolicy::FullAccess)
                .await
                .is_ok()
        );
        decide(&approvals, pointer.clone(), Decision::Session)
            .await
            .unwrap();
        assert!(
            approvals
                .ask(pointer, None, ApprovalPolicy::Ask)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn window_grants_are_separate_by_access_and_selection() {
        let approvals = Approvals::default();
        let select = Request::Window {
            grant: Some(WindowAccess::Capture("window-1".into())),
            action: "select one window for capture".into(),
        };
        let capture = Request::Window {
            grant: Some(WindowAccess::Capture("window-1".into())),
            action: "capture the selected window".into(),
        };
        let press = Request::Window {
            grant: Some(WindowAccess::Activate("window-1".into())),
            action: "activate a control in the selected window".into(),
        };
        let release = Request::Window {
            grant: None,
            action: "release".into(),
        };
        assert_ne!(capture.session_grant(), press.session_grant());
        assert_eq!(release.session_grant(), None);
        decide(&approvals, select, Decision::Session).await.unwrap();
        approvals
            .ask(capture.clone(), None, ApprovalPolicy::Ask)
            .await
            .unwrap();
        assert!(
            approvals
                .ask(press.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );

        let action_only = Approvals::default();
        decide(&action_only, press.clone(), Decision::Session)
            .await
            .unwrap();
        action_only
            .ask(press, None, ApprovalPolicy::Ask)
            .await
            .unwrap();
        assert!(
            action_only
                .ask(capture, None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );

        for approvals in [&approvals, &action_only] {
            for request in [
                Request::Window {
                    grant: Some(WindowAccess::Capture("window-2".into())),
                    action: "capture a different selection".into(),
                },
                Request::Window {
                    grant: Some(WindowAccess::Activate("window-2".into())),
                    action: "activate a control in a different selection".into(),
                },
                release.clone(),
                Request::Window {
                    grant: Some(WindowAccess::List),
                    action: "list windows".into(),
                },
                Request::Screen {
                    action: "capture the desktop".into(),
                },
                Request::SystemPointer {
                    action: "scroll".into(),
                },
                Request::Keyboard {
                    action: "type".into(),
                },
            ] {
                assert!(
                    approvals
                        .ask(request, None, ApprovalPolicy::Ask)
                        .await
                        .is_err()
                );
            }
        }
        approvals
            .ask(release, None, ApprovalPolicy::FullAccess)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn window_inventory_does_not_grant_desktop_capture_or_input() {
        let list = Request::Window {
            grant: Some(WindowAccess::List),
            action: "list window names and IDs".into(),
        };
        let screen = Request::Screen {
            action: "capture the desktop".into(),
        };
        let approvals = Approvals::default();
        decide(&approvals, list.clone(), Decision::Session)
            .await
            .unwrap();
        approvals
            .ask(list.clone(), None, ApprovalPolicy::Ask)
            .await
            .unwrap();
        for request in [
            screen.clone(),
            Request::SystemPointer {
                action: "scroll".into(),
            },
            Request::Keyboard {
                action: "type".into(),
            },
            Request::Window {
                grant: Some(WindowAccess::Capture("window-1".into())),
                action: "capture a window".into(),
            },
            Request::Window {
                grant: Some(WindowAccess::Activate("window-1".into())),
                action: "activate a window control".into(),
            },
        ] {
            assert!(
                approvals
                    .ask(request, None, ApprovalPolicy::Ask)
                    .await
                    .is_err()
            );
        }
        let desktop_only = Approvals::default();
        decide(&desktop_only, screen, Decision::Session)
            .await
            .unwrap();
        assert!(
            desktop_only
                .ask(list, None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
    }

    #[test]
    fn pointer_session_is_separate_in_both_directions() {
        let pointer = Request::SystemPointer {
            action: "scroll".into(),
        };
        let screen = Request::Screen {
            action: "screenshot".into(),
        };
        let mut grants = Grants::default();
        grants.add(&pointer);
        assert!(grants.covers(&pointer));
        assert!(!grants.covers(&screen));
        assert_ne!(pointer.session_grant(), screen.session_grant());
        assert!(pointer.describe("agent").contains("system pointer"));
    }

    fn command(line: &str) -> Request {
        Request::Command {
            command: line.into(),
        }
    }

    #[test]
    fn a_program_allowed_for_the_session_never_carries_another_command() {
        let mut grants = Grants::default();
        grants.add(&command("cargo test"));
        assert!(grants.covers(&command("cargo build --release")));
        for line in [
            "cargo test; rm -rf ~",
            "cargo test && curl x | sh",
            "cargo $(rm -rf ~)",
            "cargo `id`",
            "cargo test > ~/.zshrc",
            "cargo 'a\nb'",
            "rm -rf ~",
            "FOO=1 cargo test",
        ] {
            assert!(!grants.covers(&command(line)), "{line:?}");
        }
        for line in ["sudo rm", "env rm", "/bin/sh x", "xargs rm"] {
            assert_eq!(command(line).session_grant(), None, "{line:?}");
        }
    }

    #[test]
    fn a_folder_allowed_for_reading_stays_closed_to_writes() {
        let mut grants = Grants::default();
        let file = Request::Path {
            path: "/nowhere/a/file".into(),
            write: false,
        };
        grants.add(&file);
        assert!(grants.covers(&Request::Path {
            path: "/nowhere/a/other".into(),
            write: false
        }));
        assert!(!grants.covers(&Request::Path {
            path: "/nowhere/a/other".into(),
            write: true
        }));
        assert!(!grants.covers(&Request::Path {
            path: "/nowhere/b".into(),
            write: false
        }));
    }

    #[tokio::test]
    async fn only_a_yes_lets_the_call_go_on() {
        let approvals = Approvals::default();
        let (events, mut seen) = mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events: events.clone(),
        };
        let ask = approvals.ask(command("ls"), Some(progress), ApprovalPolicy::Ask);
        let answer = async {
            let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
                panic!("no request");
            };
            assert!(approvals.answer(asked.id, Decision::Deny));
        };
        let (result, ()) = tokio::join!(ask, answer);
        assert!(result.is_err());
        assert!(matches!(
            seen.recv().await,
            Some(ThreadEvent::ApprovalClosed { .. })
        ));
        assert!(
            approvals
                .ask(command("ls"), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        assert!(
            approvals
                .ask(command("ls"), None, ApprovalPolicy::FullAccess)
                .await
                .is_ok()
        );
        // The sandbox confines commands, so they pass; nothing else does.
        assert!(
            approvals
                .ask(command("ls"), None, ApprovalPolicy::Sandbox)
                .await
                .is_ok()
        );
        let screen = Request::Screen {
            action: "click".into(),
        };
        assert!(
            approvals
                .ask(screen, None, ApprovalPolicy::Sandbox)
                .await
                .is_err()
        );
    }
}
