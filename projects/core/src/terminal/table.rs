//! The open terminals. Each has the client that opened it for an owner; to any other
//! client its id is unknown.

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
use crate::client::ClientId;

struct Open {
    owner: ClientId,
    pty: Pty,
}

pub struct Terminals {
    shell: PathBuf,
    /// Shared with each terminal's wait thread, which takes its own entry out.
    open: Arc<Mutex<HashMap<String, Open>>>,
}

impl Terminals {
    /// Every terminal runs `shell`, a program named by its full path.
    pub fn new(shell: PathBuf) -> Self {
        Self {
            shell,
            open: Arc::default(),
        }
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
        let pty = pty::start(&self.shell, cwd, size, id.clone(), events, ended)?;
        open.insert(id.clone(), Open { owner, pty });
        info!("terminal {id} opened");
        Ok(id)
    }

    /// Answers what `caller` asks of a terminal it opened.
    pub fn handle(&self, caller: ClientId, request: TerminalRequest) -> Result<()> {
        match request {
            TerminalRequest::Open { .. } => anyhow::bail!("a terminal is opened with its queue"),
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
