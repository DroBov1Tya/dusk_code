//! A cursor of dusk's own where the agent last clicked or scrolled, so the user sees
//! where it acts. AppKit draws only on a process's main thread, which here belongs to
//! the client, so this binary runs again as a helper that owns nothing but the overlay.
//! The helper reads positions from its stdin and ends when the pipe closes.

use std::{
    io::Write,
    process::{Child, ChildStdin, Command, Stdio},
    sync::OnceLock,
};

use core_graphics::geometry::CGPoint;
use parking_lot::Mutex;
use tracing::debug;

/// The argument that makes this binary the helper; see [`super::run_overlay_if_asked`].
pub const OVERLAY_ARG: &str = "--dusk-cursor-overlay";

/// One overlay for the process, however many threads use the screen.
pub struct Cursor {
    helper: Mutex<Option<(Child, ChildStdin)>>,
}

impl Cursor {
    pub fn shared() -> &'static Self {
        static CURSOR: OnceLock<Cursor> = OnceLock::new();
        CURSOR.get_or_init(|| Self {
            helper: Mutex::new(None),
        })
    }

    /// Puts the cursor's tip at `at`, in display points. It only shows the user where the
    /// agent acts, so a helper that cannot start is no failure.
    pub fn show(&self, at: CGPoint) {
        let mut helper = self.helper.lock();
        let running = helper
            .as_mut()
            .is_some_and(|(child, _)| matches!(child.try_wait(), Ok(None)));
        if !running {
            *helper = start();
        }
        let Some((_, stdin)) = helper.as_mut() else {
            return;
        };
        if let Err(error) = writeln!(stdin, "{} {}", at.x, at.y).and_then(|()| stdin.flush()) {
            debug!("cannot place the cursor: {error}");
            *helper = None;
        }
    }
}

fn start() -> Option<(Child, ChildStdin)> {
    let started = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg(OVERLAY_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    });
    match started {
        Ok(mut child) => child.stdin.take().map(|stdin| (child, stdin)),
        Err(error) => {
            debug!("cannot start the cursor: {error}");
            None
        }
    }
}
