//! The iOS runtime download: several gigabytes, one at a time for the whole process.

use parking_lot::Mutex;

use super::run::{command, last_line};

struct Download {
    running: bool,
    failed: Option<String>,
}

static DOWNLOAD: Mutex<Download> = Mutex::new(Download {
    running: false,
    failed: None,
});

/// Whether a download runs, and why the last one failed.
pub fn state() -> (bool, Option<String>) {
    let download = DOWNLOAD.lock();
    (download.running, download.failed.clone())
}

pub fn start() {
    {
        let mut download = DOWNLOAD.lock();
        if download.running {
            return;
        }
        download.running = true;
        download.failed = None;
    }
    tokio::spawn(async move {
        let mut xcodebuild = command("xcodebuild");
        xcodebuild.args(["-downloadPlatform", "iOS"]);
        let failed = match xcodebuild.output().await {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(last_line(&output.stderr)),
            Err(error) => Some(format!("xcodebuild did not start: {error}")),
        };
        let mut download = DOWNLOAD.lock();
        download.running = false;
        download.failed = failed;
    });
}
