//! Resources missing at start are fetched before the screen is taken, so their progress
//! is shown on the plain terminal: one line, redrawn in place when it is a terminal.

use std::io::{self, IsTerminal, Write};

use dusk_core::release::{
    complete::{self, Step},
    update,
};
use tracing::{info, warn};

pub async fn ensure() {
    let terminal = io::stderr().is_terminal();
    let shown = |step: Step| {
        let line = match step {
            Step::Listing => format!(
                "duskcode: resources for {} are missing, asking GitHub for them",
                update::VERSION
            ),
            Step::Fetching { done, total } => {
                format!("duskcode: fetching resources {done}/{total}")
            }
        };
        let mut stderr = io::stderr().lock();
        // Off a terminal, the fetch is one line at its start and one at its end.
        let _ = match (terminal, step) {
            (true, Step::Listing) => writeln!(stderr, "{line}"),
            (true, _) => write!(stderr, "\r\x1b[2K{line}"),
            (false, Step::Fetching { done, total }) if done != 0 && done != total => Ok(()),
            (false, _) => writeln!(stderr, "{line}"),
        };
        let _ = stderr.flush();
    };
    match complete::ensure_installed(&shown).await {
        Ok(None) => {}
        Ok(Some(fetched)) => {
            if terminal {
                eprintln!();
            }
            info!("resources restored from GitHub, {fetched} files");
        }
        Err(error) => {
            if terminal {
                eprintln!();
            }
            eprintln!("duskcode: cannot fetch the resources, starting without them: {error:#}");
            warn!("cannot restore the resources: {error:#}");
        }
    }
}
