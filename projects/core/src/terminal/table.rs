//! The open terminals. Each has the client that opened it for an owner; to any other
//! client its id is unknown. The agent reaches only the terminals of the app on this
//! machine (see `set_app`), through the `terminal` tool, which asks before it types.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::client::{
    Event, MAX_TERMINAL_INPUT, MAX_TERMINAL_SIDE, MAX_TERMINALS, TerminalRequest,
};
use parking_lot::Mutex;
use portable_pty::PtySize;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

use super::pty::{self, Pty};
use super::tail::Tail;
use crate::client::ClientId;

struct Open {
    owner: ClientId,
    pty: Pty,
    /// The folder the shell started in, for when the system cannot tell where it is now.
    cwd: PathBuf,
    tail: Arc<Mutex<Tail>>,
}

/// A terminal of the app as the agent sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub id: String,
    /// What a program set as the title, or the folder's name.
    pub title: String,
    pub cwd: PathBuf,
    /// A program other than the shell holds it, such as a dev server.
    pub busy: bool,
}

/// The client that is the app on this machine, with its queue of events.
type App = (ClientId, UnboundedSender<Event>);

pub struct Terminals {
    shell: PathBuf,
    /// Shared with each terminal's wait thread, which takes its own entry out.
    open: Arc<Mutex<HashMap<String, Open>>>,
    app: Mutex<Option<App>>,
}

impl Terminals {
    /// Every terminal runs `shell`, a program named by its full path.
    pub fn new(shell: PathBuf) -> Self {
        Self {
            shell,
            open: Arc::default(),
            app: Mutex::new(None),
        }
    }

    /// The client that is the app on this machine, whose terminals the agent may reach;
    /// `None` when it went away.
    pub fn set_app(&self, app: Option<App>) {
        *self.app.lock() = app;
    }

    /// Starts the shell in `cwd`, which the caller has checked, for `owner`; `events` is
    /// the owner's own queue. Answers with the new terminal's id.
    pub fn open(
        &self,
        owner: ClientId,
        cwd: &Path,
        cols: u16,
        rows: u16,
        events: UnboundedSender<Event>,
    ) -> Result<String> {
        self.start(owner, cwd, (cols, rows), events, false)
    }

    /// `open`; with `announce` the owner is told `TerminalOpened` before the shell's first
    /// output, and `TerminalClosed` when it fails to start.
    fn start(
        &self,
        owner: ClientId,
        cwd: &Path,
        (cols, rows): (u16, u16),
        events: UnboundedSender<Event>,
        announce: bool,
    ) -> Result<String> {
        // Held until the terminal is in the table, so a shell that ends at once finds
        // its entry there to take out.
        let mut open = self.open.lock();
        let owned = open.values().filter(|open| open.owner == owner).count();
        ensure!(
            owned < MAX_TERMINALS,
            "at most {MAX_TERMINALS} terminals can be open at once"
        );
        let id = uuid::Uuid::now_v7().to_string();
        let ended = {
            let (table, id) = (Arc::clone(&self.open), id.clone());
            move || {
                table.lock().remove(&id);
            }
        };
        let size = size(cols, rows);
        let tail = Arc::default();
        if announce {
            let _ = events.send(Event::TerminalOpened {
                id: id.clone(),
                cwd: cwd.to_owned(),
            });
        }
        let told = events.clone();
        let started = pty::start(
            &self.shell,
            cwd,
            size,
            id.clone(),
            events,
            Arc::clone(&tail),
            ended,
        );
        let pty = match started {
            Ok(pty) => pty,
            Err(error) => {
                if announce {
                    let closed = Event::TerminalClosed {
                        id: id.clone(),
                        code: None,
                    };
                    let _ = told.send(closed);
                }
                return Err(error);
            }
        };
        let entry = Open {
            owner,
            pty,
            cwd: cwd.to_owned(),
            tail,
        };
        open.insert(id.clone(), entry);
        info!("terminal {id} opened");
        Ok(id)
    }

    /// Answers what `caller` asks of a terminal it opened.
    pub fn handle(&self, caller: ClientId, request: TerminalRequest) -> Result<()> {
        match request {
            TerminalRequest::Open { .. } | TerminalRequest::Host => {
                anyhow::bail!("answered by the server, which knows the client's queue")
            }
            TerminalRequest::Input { id, data } => {
                ensure!(
                    data.len() <= MAX_TERMINAL_INPUT,
                    "one input is at most {MAX_TERMINAL_INPUT} bytes"
                );
                owned(&self.open.lock(), caller, &id)?.pty.input(data)
            }
            TerminalRequest::Resize { id, cols, rows } => owned(&self.open.lock(), caller, &id)?
                .pty
                .resize(size(cols, rows)),
            TerminalRequest::Close { id } => {
                let mut open = self.open.lock();
                owned(&open, caller, &id)?;
                let closed = open.remove(&id).context("no such terminal")?;
                closed.pty.hang_up();
                Ok(())
            }
        }
    }

    /// Ends every terminal of a client that went away.
    pub fn close_client(&self, owner: ClientId) {
        let gone: Vec<Open> = self
            .open
            .lock()
            .extract_if(|_, open| open.owner == owner)
            .map(|(_, open)| open)
            .collect();
        for open in gone {
            open.pty.hang_up();
        }
    }

    /// Ends every terminal, as the core stops.
    pub fn close_all(&self) {
        let all: Vec<Open> = self.open.lock().drain().map(|(_, open)| open).collect();
        for open in all {
            open.pty.hang_up();
        }
    }
}

/// What the agent asks of the app's terminals. An id of another client's terminal is
/// as unknown here as one that was never there.
impl Terminals {
    fn app(&self) -> Result<App> {
        self.app
            .lock()
            .clone()
            .context("no terminal open here: no app window with a terminal panel is attached")
    }

    /// The app's terminals, oldest first.
    pub fn shown(&self) -> Result<Vec<Shown>> {
        let (app, _) = self.app()?;
        let open = self.open.lock();
        let mut shown: Vec<Shown> = open
            .iter()
            .filter(|(_, open)| open.owner == app)
            .map(|(id, open)| shown(id, open))
            .collect();
        // Ids are UUIDv7: in the order they were made.
        shown.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(shown)
    }

    /// One of the app's terminals, with its last `lines` lines of output.
    pub fn read(&self, id: &str, lines: usize) -> Result<(Shown, Vec<String>)> {
        let (app, _) = self.app()?;
        let open = self.open.lock();
        let found = owned(&open, app, id)?;
        let lines = found.tail.lock().lines(lines);
        Ok((shown(id, found), lines))
    }

    /// One of the app's terminals, as `read` without the output.
    pub fn find(&self, id: &str) -> Result<Shown> {
        let (app, _) = self.app()?;
        let open = self.open.lock();
        Ok(shown(id, owned(&open, app, id)?))
    }

    /// Types `data` into one of the app's terminals; the caller asked the user first.
    pub fn type_in(&self, id: &str, data: Vec<u8>) -> Result<()> {
        let (app, _) = self.app()?;
        ensure!(
            data.len() <= MAX_TERMINAL_INPUT,
            "one input is at most {MAX_TERMINAL_INPUT} bytes"
        );
        owned(&self.open.lock(), app, id)?.pty.input(data)
    }

    /// Opens a terminal in the app, in `cwd`, which the caller has checked; the app is
    /// told first, so it has the tab before the shell's first output comes.
    pub fn open_in_app(&self, cwd: &Path) -> Result<String> {
        let (app, events) = self.app()?;
        self.start(app, cwd, (80, 24), events, true)
    }
}

/// A terminal as it is now: its folder is where the shell went with `cd`, so one moved
/// out of the project counts as outside it.
fn shown(id: &str, open: &Open) -> Shown {
    let title = open.tail.lock().title().map(str::to_owned);
    let cwd = open.pty.cwd().unwrap_or_else(|| open.cwd.clone());
    let folder = || {
        cwd.file_name().map_or_else(
            || cwd.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    };
    Shown {
        id: id.to_owned(),
        title: title.unwrap_or_else(folder),
        cwd: cwd.clone(),
        busy: open.pty.busy(),
    }
}

/// The terminal `id` when `caller` opened it. One that another client opened is as
/// unknown as one that was never there, so an id tells nothing to who guesses it.
fn owned<'a>(open: &'a HashMap<String, Open>, caller: ClientId, id: &str) -> Result<&'a Open> {
    open.get(id)
        .filter(|open| open.owner == caller)
        .context("no such terminal")
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.clamp(1, MAX_TERMINAL_SIDE),
        rows: rows.clamp(1, MAX_TERMINAL_SIDE),
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
impl Terminals {
    fn pid(&self, id: &str) -> Option<u32> {
        self.open.lock().get(id)?.pty.pid()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::time::Duration;
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    const WAIT: Duration = Duration::from_secs(20);

    /// A terminal of client 1 running `/bin/sh` in a fresh folder, or none where this
    /// machine gives no pseudo-terminal, as in some sandboxes: then the test is skipped.
    fn open(terminals: &Terminals, name: &str) -> Option<(String, UnboundedReceiver<Event>)> {
        let cwd = temp_dir(name);
        std::fs::create_dir_all(&cwd).unwrap();
        let (queue, events) = unbounded_channel();
        match terminals.open(1, &cwd, 80, 24, queue) {
            Ok(id) => Some((id, events)),
            Err(error) if format!("{error:#}").contains("pseudo-terminal") => {
                eprintln!("skipped: {error:#}");
                None
            }
            Err(error) => panic!("{error:#}"),
        }
    }

    fn input(id: &str, text: &str) -> TerminalRequest {
        TerminalRequest::Input {
            id: id.to_owned(),
            data: text.as_bytes().to_vec(),
        }
    }

    /// What the terminal printed until it closed, and the exit code it closed with.
    async fn until_closed(
        events: &mut UnboundedReceiver<Event>,
        id: &str,
    ) -> (String, Option<i32>) {
        let mut printed = Vec::new();
        loop {
            let event = tokio::time::timeout(WAIT, events.recv()).await;
            match event.expect("the terminal did not close").unwrap() {
                Event::Terminal { id: from, data } if from == id => {
                    assert!(data.len() <= 64 * 1024);
                    printed.extend(data);
                }
                Event::TerminalClosed { id: from, code } if from == id => {
                    return (String::from_utf8_lossy(&printed).into_owned(), code);
                }
                other => panic!("an event of another terminal: {other:?}"),
            }
        }
    }

    fn alive(pid: u32) -> bool {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
    }

    #[tokio::test]
    async fn a_shell_prints_to_its_client_and_closes_when_it_exits() {
        let terminals = Terminals::new("/bin/sh".into());
        let Some((id, mut events)) = open(&terminals, "terminal-run") else {
            return;
        };
        // The echo of what is typed has `h%s`; only the shell's own output has `hi`.
        let typed = input(&id, "printf 'h%s\\n' i; exit 3\n");
        terminals.handle(1, typed).unwrap();
        let (printed, code) = until_closed(&mut events, &id).await;
        assert!(printed.contains("hi"), "{printed:?}");
        assert_eq!(code, Some(3));
        // The id is gone with the shell.
        assert!(terminals.handle(1, input(&id, "x")).is_err());
        assert!(terminals.open.lock().is_empty());
    }

    /// A terminal's folder is where its shell is now, so a `cd` out of the project moves
    /// it outside for the `terminal` tool's approval.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn a_terminal_is_where_its_shell_went() {
        let terminals = Terminals::new("/bin/sh".into());
        let (queue, _events) = unbounded_channel();
        terminals.set_app(Some((1, queue)));
        let Some((id, _own)) = open(&terminals, "terminal-cd") else {
            return;
        };
        let started = terminals.find(&id).unwrap().cwd;
        let away = temp_dir("terminal-cd-away");
        std::fs::create_dir_all(&away).unwrap();
        let away = away.canonicalize().unwrap();
        let typed = input(&id, &format!("cd '{}'\n", away.display()));
        terminals.handle(1, typed).unwrap();
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            let now = terminals.find(&id).unwrap().cwd;
            if now.canonicalize().ok() == Some(away.clone()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "still in {now:?}, from {started:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        terminals.close_all();
    }

    #[tokio::test]
    async fn a_terminal_is_unknown_to_a_client_that_did_not_open_it() {
        let terminals = Terminals::new("/bin/sh".into());
        let Some((id, mut events)) = open(&terminals, "terminal-owner") else {
            return;
        };
        let (cols, rows) = (10, 10);
        let unknown = terminals.handle(1, input("no-such-id", "x")).unwrap_err();
        for request in [
            input(&id, "touch stolen\n"),
            TerminalRequest::Resize {
                id: id.clone(),
                cols,
                rows,
            },
            TerminalRequest::Close { id: id.clone() },
        ] {
            let refused = terminals.handle(2, request).unwrap_err();
            assert_eq!(refused.to_string(), unknown.to_string());
        }
        let large = TerminalRequest::Input {
            id: id.clone(),
            data: vec![b' '; MAX_TERMINAL_INPUT + 1],
        };
        assert!(terminals.handle(1, large).is_err());

        // Still the owner's, and untouched by the other client.
        let resize = TerminalRequest::Resize {
            id: id.clone(),
            cols: 0,
            rows: u16::MAX,
        };
        terminals.handle(1, resize).unwrap();
        terminals.handle(1, input(&id, "ls; exit\n")).unwrap();
        let (printed, _) = until_closed(&mut events, &id).await;
        assert!(!printed.contains("stolen"), "{printed:?}");
    }

    #[tokio::test]
    async fn a_client_that_goes_away_takes_its_shells_with_it() {
        let terminals = Terminals::new("/bin/sh".into());
        let Some((id, mut events)) = open(&terminals, "terminal-gone") else {
            return;
        };
        let Some((other, mut other_events)) = open(&terminals, "terminal-stays") else {
            return;
        };
        let pid = terminals.pid(&id).unwrap();
        assert!(alive(pid));
        // Client 2 leaving ends nothing of client 1.
        terminals.close_client(2);
        assert!(alive(pid));
        let closing = TerminalRequest::Close { id: other.clone() };
        terminals.handle(1, closing).unwrap();
        until_closed(&mut other_events, &other).await;
        assert!(alive(pid));

        terminals.close_client(1);
        assert!(terminals.handle(1, input(&id, "x")).is_err());
        // Closed is told once the shell is reaped, so the process is gone by then.
        until_closed(&mut events, &id).await;
        assert!(!alive(pid));
    }

    #[tokio::test]
    async fn a_client_has_a_limit_of_open_terminals() {
        let terminals = Terminals::new("/bin/sh".into());
        let mut kept = Vec::new();
        for _ in 0..MAX_TERMINALS {
            let Some(open) = open(&terminals, "terminal-limit") else {
                return;
            };
            kept.push(open);
        }
        let cwd = temp_dir("terminal-over");
        std::fs::create_dir_all(&cwd).unwrap();
        let (queue, _events) = unbounded_channel();
        assert!(terminals.open(1, &cwd, 80, 24, queue.clone()).is_err());
        // The limit is each client's own.
        let theirs = terminals.open(2, &cwd, 80, 24, queue).unwrap();
        assert!(terminals.pid(&theirs).is_some());
        terminals.close_all();
        for (id, mut events) in kept {
            until_closed(&mut events, &id).await;
        }
    }
}
