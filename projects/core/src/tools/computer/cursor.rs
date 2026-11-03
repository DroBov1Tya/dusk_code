//! A cursor of dusk's own where the agent last clicked or scrolled, so the user sees
//! where it acts. A small helper draws it: osascript running a few lines of AppKit, a
//! ring above every window that the mouse passes through and screenshots leave out.

use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use core_graphics::geometry::CGPoint;
use parking_lot::Mutex;
use tracing::debug;

const SCRIPT: &str = include_str!("cursor.js");

pub struct Cursor {
    helper: Mutex<Option<Child>>,
    /// Where the helper reads the position; removing it ends the helper.
    file: PathBuf,
}

impl Cursor {
    pub fn new() -> Self {
        let name = format!(
            "duskcode-cursor-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        );
        Self {
            helper: Mutex::new(None),
            file: std::env::temp_dir().join(name),
        }
    }

    /// Puts the ring at `at`, in display points. It only shows the user where the agent
    /// acts, so a helper that cannot start is no failure.
    pub fn show(&self, at: CGPoint) {
        let staged = self.file.with_extension("new");
        let written = fs::write(&staged, format!("{} {}\n", at.x, at.y))
            .and_then(|()| fs::rename(&staged, &self.file));
        if let Err(error) = written {
            debug!("cannot place the cursor: {error}");
            return;
        }
        let mut helper = self.helper.lock();
        let running = helper
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if running {
            return;
        }
        match Command::new("osascript")
            .args(["-l", "JavaScript", "-e", SCRIPT])
            .arg(&self.file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => *helper = Some(child),
            Err(error) => debug!("cannot start the cursor: {error}"),
        }
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        if let Some(mut child) = self.helper.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
