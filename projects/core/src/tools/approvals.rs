//! Asking the user before a tool leaves its folders or runs a command. The call waits for
//! the answer, shown by the client; full access answers yes at once. A yes for the
//! session lasts until the core stops and is never written down.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;

use anyhow::{Context, Result, bail};
use tokio::sync::{mpsc, oneshot};

use super::{
    console::command_line::{WRAPPERS, program_name},
    tool::Progress,
};
use crate::{config::ApprovalPolicy, runtime::ThreadEvent};

pub use dusk_protocol::tools::{ApprovalRequest, ApprovalStep, Request, WindowAccess};

pub use dusk_protocol::tools::Decision;

/// What decides a request in the user's place, in arbiter mode.
pub trait Judge: Send + Sync {
    /// An error leaves the request to the user.
    fn judge<'a>(&'a self, request: &'a Request) -> crate::BoxFuture<'a, Verdict>;
}

pub struct Verdict {
    pub allow: bool,
    pub reason: String,
}

/// The thread that asked, whether only the owner may answer, and where the answer goes.
type Pending = (String, bool, oneshot::Sender<Decision>);

#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    /// Each open request with the thread that asked it, and whether only the owner, a
    /// client that may change the settings itself, may answer it.
    pending: Mutex<HashMap<u64, Pending>>,
    grants: Mutex<Grants>,
    judge: std::sync::OnceLock<std::sync::Arc<dyn Judge>>,
}

impl Approvals {
    /// A set of its own whose request numbers begin at `first`, so two sets never give
    /// the same number to a client that answers by it.
    pub fn starting_at(first: u64) -> Self {
        Self {
            next: AtomicU64::new(first),
            ..Self::default()
        }
    }

    pub fn set_judge(&self, judge: std::sync::Arc<dyn Judge>) {
        let _ = self.judge.set(judge);
    }

    /// Waits for the user's answer; an error unless it is yes. Without `progress` no
    /// client listens, so only full access or a session grant lets the request through.
    pub async fn ask(
        &self,
        thread_id: &str,
        request: Request,
        progress: Option<Progress>,
        policy: ApprovalPolicy,
    ) -> Result<()> {
        // In sandbox mode a command needs no yes: the system confines it when it runs. Work
        // on an agent's own branch is bounded the same way, by the core: what reaches a
        // branch of the user's is asked with `ask_always`.
        let confined = policy == ApprovalPolicy::Sandbox
            && matches!(request, Request::Command { .. } | Request::Git { .. });
        if policy == ApprovalPolicy::FullAccess || confined || self.grants.lock().covers(&request) {
            return Ok(());
        }
        if policy == ApprovalPolicy::Arbiter {
            return self.arbitrate(thread_id, request, progress).await;
        }
        let session = session_grant(&request);
        self.prompt(thread_id, request, session, Vec::new(), progress)
            .await
    }

    /// Asks the owner whatever the mode and the session's grants say, offering no grant
    /// for the session: a change of the settings, which a tool may only propose. Only a
    /// client that could make the change itself may answer (`owner_only`), so a chat
    /// shared with someone else never lets them make it.
    pub async fn ask_owner(
        &self,
        thread_id: &str,
        request: Request,
        progress: Option<Progress>,
    ) -> Result<()> {
        self.open(thread_id, request, None, Vec::new(), progress, true)
            .await
    }

    /// Asks the user whatever the mode and the session's grants say, offering no grant
    /// for the session: a step that cannot be taken back the way it was asked, such as a
    /// merge into the user's own branch. Any client that answers requests may answer it.
    pub async fn ask_always(
        &self,
        thread_id: &str,
        request: Request,
        progress: Option<Progress>,
    ) -> Result<()> {
        self.open(thread_id, request, None, Vec::new(), progress, false)
            .await
    }

    /// Whether only the owner may answer request `id`.
    pub fn owner_only(&self, id: u64) -> bool {
        self.pending
            .lock()
            .get(&id)
            .is_some_and(|(_, owner, _)| *owner)
    }

    /// Screen actions asked as one: never less than asking each alone. A request the
    /// session grants already is not asked again but still listed, with its number in
    /// the batch, so the user sees every step. Steps of one kind may be allowed for the
    /// session as that kind; mixed kinds are asked as the strictest of them for this
    /// time only, since one session yes must not stand for another kind's. The arbiter
    /// judges each step on its own.
    pub async fn ask_all(
        &self,
        thread_id: &str,
        requests: Vec<Request>,
        progress: Option<Progress>,
        policy: ApprovalPolicy,
    ) -> Result<()> {
        if policy == ApprovalPolicy::FullAccess {
            return Ok(());
        }
        if requests.len() < 2 || requests.iter().any(|r| strictness(r).is_none()) {
            // A single request looks as it always has; only screen kinds merge.
            for request in requests {
                self.ask(thread_id, request, progress.clone(), policy)
                    .await?;
            }
            return Ok(());
        }
        let steps: Vec<ApprovalStep> = {
            let grants = self.grants.lock();
            requests
                .into_iter()
                .enumerate()
                .map(|(index, request)| ApprovalStep {
                    number: index + 1,
                    allowed: grants.covers(&request),
                    request,
                })
                .collect()
        };
        let open: Vec<&ApprovalStep> = steps.iter().filter(|step| !step.allowed).collect();
        let Some(first) = open.first().map(|step| &step.request) else {
            return Ok(());
        };
        if policy == ApprovalPolicy::Arbiter {
            for step in &open {
                let judged = self.arbitrate(thread_id, step.request.clone(), progress.clone());
                judged
                    .await
                    .with_context(|| format!("step {} of the batch", step.number))?;
            }
            return Ok(());
        }
        let same = open.iter().all(|step| same_kind(&step.request, first));
        let strictest = open
            .iter()
            .map(|step| &step.request)
            .max_by_key(|r| strictness(r))
            .unwrap_or(first);
        // The steps go in the text too, so whatever shows only the request, such as a
        // message on a chat platform, still shows each step it allows.
        let lines: Vec<String> = steps.iter().map(ApprovalStep::line).collect();
        let action = format!("{} computer actions:\n{}", steps.len(), lines.join("\n"));
        let request = with_action(strictest, action);
        let session = if same { session_grant(&request) } else { None };
        self.prompt(thread_id, request, session, steps, progress)
            .await
    }

    /// Command lines asked as one list, for a yes that covers exactly these lines: the
    /// gates of a plan, which run later where nobody can be asked. A line the session
    /// allows already is listed but not asked. The session is never offered, and the
    /// caller asks only in a mode that would ask for each command.
    pub async fn ask_commands(
        &self,
        thread_id: &str,
        commands: &[String],
        progress: Option<Progress>,
    ) -> Result<()> {
        let steps: Vec<ApprovalStep> = {
            let grants = self.grants.lock();
            commands
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    let request = Request::Command {
                        command: command.clone(),
                    };
                    ApprovalStep {
                        number: index + 1,
                        allowed: grants.covers(&request),
                        request,
                    }
                })
                .collect()
        };
        if steps.iter().all(|step| step.allowed) {
            return Ok(());
        }
        let request = Request::Command {
            command: commands.join("\n"),
        };
        self.prompt(thread_id, request, None, steps, progress).await
    }

    /// Shows the request to the user and waits. A yes for the session
    /// adds a grant only when `session` offered one.
    async fn prompt(
        &self,
        thread_id: &str,
        request: Request,
        session: Option<String>,
        steps: Vec<ApprovalStep>,
        progress: Option<Progress>,
    ) -> Result<()> {
        self.open(thread_id, request, session, steps, progress, false)
            .await
    }

    async fn open(
        &self,
        thread_id: &str,
        request: Request,
        session: Option<String>,
        steps: Vec<ApprovalStep>,
        progress: Option<Progress>,
        owner_only: bool,
    ) -> Result<()> {
        let Some(progress) = progress else {
            let what = match steps.is_empty() {
                true => request.what(),
                false => steps
                    .iter()
                    .filter(|step| !step.allowed)
                    .map(ApprovalStep::line)
                    .collect::<Vec<_>>()
                    .join("; "),
            };
            bail!("{what} needs the user's approval, and no one can answer here");
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, answer) = oneshot::channel();
        self.pending
            .lock()
            .insert(id, (thread_id.to_owned(), owner_only, sender));
        // Closes the request on the client however the wait ends, a stopped turn included.
        let _open = Open {
            approvals: self,
            id,
            events: progress.events.clone(),
        };
        let offered = session.is_some();
        progress.emit(ThreadEvent::Approval(ApprovalRequest {
            id,
            request: request.clone(),
            session,
            steps,
        }));
        match answer.await {
            Ok(Decision::Once) => Ok(()),
            Ok(Decision::Session) => {
                if offered {
                    self.grants.lock().add(&request);
                }
                Ok(())
            }
            Ok(Decision::Deny) | Err(_) => bail!(
                "the user denied {}; do not try it another way: ask them, or go on without it",
                request.what()
            ),
        }
    }

    /// The arbiter's verdict stands for the user's answer; the thread that asked hears it.
    async fn arbitrate(
        &self,
        thread_id: &str,
        request: Request,
        progress: Option<Progress>,
    ) -> Result<()> {
        let what = request.what();
        let verdict = match self.judge.get() {
            Some(judge) => judge.judge(&request).await,
            None => Err(anyhow::anyhow!("no arbiter is running")),
        };
        // An unclear answer or a failure is not a no the user gave: they are asked.
        let Ok(verdict) = verdict else {
            let session = session_grant(&request);
            return self
                .prompt(thread_id, request, session, Vec::new(), progress)
                .await;
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
            .is_some_and(|(_, _, sender)| sender.send(decision).is_ok())
    }

    /// The thread that asked request `id`; `None` when nothing waits for it.
    pub fn thread_of(&self, id: u64) -> Option<String> {
        self.pending
            .lock()
            .get(&id)
            .map(|(thread, _, _)| thread.clone())
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
    /// Work on the agents' own branches.
    git: bool,
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
            Request::Keyboard { .. } | Request::Settings { .. } => false,
            Request::Window { grant, .. } => grant
                .as_ref()
                .is_some_and(|access| self.windows.contains(access)),
            Request::Git { .. } => self.git,
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
            Request::Keyboard { .. } | Request::Settings { .. } => {}
            Request::Window { grant, .. } => self.windows.extend(grant.clone()),
            Request::Git { .. } => self.git = true,
        }
    }
}

/// How much a screen request can do, for a merged request to take the strictest kind.
/// `None` for requests that are not about the screen.
fn strictness(request: &Request) -> Option<u8> {
    match request {
        Request::Screen { .. } => Some(0),
        Request::Window { .. } => Some(1),
        Request::SystemPointer { .. } => Some(2),
        Request::Keyboard { .. } => Some(3),
        Request::Path { .. }
        | Request::Command { .. }
        | Request::Settings { .. }
        | Request::Git { .. } => None,
    }
}

/// Whether one session grant would cover both.
fn same_kind(a: &Request, b: &Request) -> bool {
    match (a, b) {
        (Request::Window { grant: a, .. }, Request::Window { grant: b, .. }) => a == b,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// `kind` asking for `action` instead.
fn with_action(kind: &Request, action: String) -> Request {
    match kind {
        Request::Screen { .. } => Request::Screen { action },
        Request::SystemPointer { .. } => Request::SystemPointer { action },
        Request::Keyboard { .. } => Request::Keyboard { action },
        Request::Window { grant, .. } => Request::Window {
            grant: grant.clone(),
            action,
        },
        Request::Path { .. }
        | Request::Command { .. }
        | Request::Settings { .. }
        | Request::Git { .. } => kind.clone(),
    }
}

/// What a yes for the session covers, for the client to name: the folder of the path,
/// or the program of a plain command. `None` when the session cannot be offered.
pub fn session_grant(request: &Request) -> Option<String> {
    match request {
        Request::Path { path, .. } => Some(folder_of(path).display().to_string()),
        Request::Command { command } => program(command).map(str::to_owned),
        Request::Screen { .. } => Some("the screen".into()),
        Request::SystemPointer { .. } => Some("the system pointer".into()),
        Request::Keyboard { .. } | Request::Settings { .. } => None,
        Request::Window { grant, .. } => match grant {
            Some(WindowAccess::List) => Some("listing window names and IDs".into()),
            Some(WindowAccess::Capture(_)) => Some("capture of this selected window".into()),
            Some(WindowAccess::Activate(_)) => {
                Some("control activation in this selected window".into())
            }
            None => None,
        },
        Request::Git { .. } => Some("work on the agents' own branches".into()),
    }
}

/// Characters that let one command line run another, or hide what it runs. A program
/// allowed for the session is allowed only on lines without them.
const SHELL_SYNTAX: &[char] = &[
    ';', '&', '|', '$', '`', '<', '>', '(', ')', '\n', '\r', '\'', '"', '\\',
];

/// Programs that run their arguments as another command; allowing one would allow all.
/// The wrappers a command line is read through (`WRAPPERS`) are runners too.
const RUNNERS: &[&str] = &[
    "xargs", "eval", "watch", "sh", "bash", "zsh", "fish", "dash", "ksh", "su",
];

fn is_runner(name: &str) -> bool {
    RUNNERS.contains(&name) || WRAPPERS.contains(&name)
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
    (plain && !is_runner(program_name(name))).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn decide(approvals: &Approvals, request: Request, decision: Decision) -> Result<()> {
        let (events, mut seen) = mpsc::unbounded_channel();
        let ask = approvals.ask(
            "t",
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
                .ask("t", screen, None, ApprovalPolicy::Ask)
                .await
                .is_ok()
        );
        for policy in [
            ApprovalPolicy::Ask,
            ApprovalPolicy::Sandbox,
            ApprovalPolicy::Arbiter,
        ] {
            assert!(
                approvals
                    .ask("t", pointer.clone(), None, policy)
                    .await
                    .is_err(),
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
                .ask("t", pointer.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        decide(&approvals, pointer.clone(), Decision::Once)
            .await
            .unwrap();
        assert!(
            approvals
                .ask("t", pointer.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        // FullAccess is an explicit global policy, not an inferred screen grant.
        assert!(
            approvals
                .ask("t", pointer.clone(), None, ApprovalPolicy::FullAccess)
                .await
                .is_ok()
        );
        decide(&approvals, pointer.clone(), Decision::Session)
            .await
            .unwrap();
        assert!(
            approvals
                .ask("t", pointer, None, ApprovalPolicy::Ask)
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
        assert_ne!(session_grant(&capture), session_grant(&press));
        assert_eq!(session_grant(&release), None);
        decide(&approvals, select, Decision::Session).await.unwrap();
        approvals
            .ask("t", capture.clone(), None, ApprovalPolicy::Ask)
            .await
            .unwrap();
        assert!(
            approvals
                .ask("t", press.clone(), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );

        let action_only = Approvals::default();
        decide(&action_only, press.clone(), Decision::Session)
            .await
            .unwrap();
        action_only
            .ask("t", press, None, ApprovalPolicy::Ask)
            .await
            .unwrap();
        assert!(
            action_only
                .ask("t", capture, None, ApprovalPolicy::Ask)
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
                        .ask("t", request, None, ApprovalPolicy::Ask)
                        .await
                        .is_err()
                );
            }
        }
        approvals
            .ask("t", release, None, ApprovalPolicy::FullAccess)
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
            .ask("t", list.clone(), None, ApprovalPolicy::Ask)
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
                    .ask("t", request, None, ApprovalPolicy::Ask)
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
                .ask("t", list, None, ApprovalPolicy::Ask)
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
        assert_ne!(session_grant(&pointer), session_grant(&screen));
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
            assert_eq!(session_grant(&command(line)), None, "{line:?}");
        }
    }

    /// The runners stay refused now that the wrappers come from the command-line reader.
    #[test]
    fn no_runner_is_ever_allowed_for_the_session() {
        for runner in [
            "sudo", "doas", "env", "xargs", "exec", "eval", "command", "builtin", "nohup", "nice",
            "timeout", "time", "watch", "sh", "bash", "zsh", "fish", "dash", "ksh", "su",
        ] {
            for line in [
                format!("{runner} cargo test"),
                format!("/usr/bin/{runner} ls"),
            ] {
                assert_eq!(session_grant(&command(&line)), None, "{line:?}");
            }
        }
        for wrapper in WRAPPERS {
            assert_eq!(session_grant(&command(&format!("{wrapper} ls"))), None);
        }
        assert_eq!(
            session_grant(&command("cargo test")).as_deref(),
            Some("cargo")
        );
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
        let ask = approvals.ask("t", command("ls"), Some(progress), ApprovalPolicy::Ask);
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
                .ask("t", command("ls"), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        assert!(
            approvals
                .ask("t", command("ls"), None, ApprovalPolicy::FullAccess)
                .await
                .is_ok()
        );
        // The sandbox confines commands, so they pass; nothing else does.
        assert!(
            approvals
                .ask("t", command("ls"), None, ApprovalPolicy::Sandbox)
                .await
                .is_ok()
        );
        let screen = Request::Screen {
            action: "click".into(),
        };
        assert!(
            approvals
                .ask("t", screen, None, ApprovalPolicy::Sandbox)
                .await
                .is_err()
        );
    }

    /// Branch work passes as the mode says, like a command; what reaches the user's own
    /// branch is asked every time: no mode, grant or session yes lets it through unasked.
    #[tokio::test]
    async fn the_users_branch_is_asked_in_every_mode() {
        let approvals = Approvals::default();
        let git = |action: &str| Request::Git {
            action: action.into(),
        };
        for policy in [ApprovalPolicy::Sandbox, ApprovalPolicy::FullAccess] {
            assert!(
                approvals
                    .ask("t", git("commit"), None, policy)
                    .await
                    .is_ok()
            );
        }
        assert!(
            approvals
                .ask("t", git("commit"), None, ApprovalPolicy::Ask)
                .await
                .is_err()
        );
        decide(&approvals, git("start"), Decision::Session)
            .await
            .unwrap();
        assert!(
            approvals
                .ask("t", git("commit"), None, ApprovalPolicy::Ask)
                .await
                .is_ok(),
            "the session's yes holds for branch work"
        );
        assert!(
            approvals
                .ask_always("t", git("merge into main"), None)
                .await
                .is_err(),
            "nobody to ask, so no merge"
        );
    }
}
