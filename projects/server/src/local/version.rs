//! What an app does when the core it meets has another version.
//!
//! An older app never stops a newer core. A newer app stops an idle background `serve`
//! silently, asks before it restarts a busy one, and asks a desktop or terminal owner to
//! hand over. `decide` takes only what a core that proved itself told; a `Refused` received
//! before the proof is displayed by the caller and never reaches it.

use std::cmp::Ordering;
use std::path::PathBuf;

use dusk_protocol::client::{CoreInfo, OwnerKind};

use super::LockInfo;

/// The step an app takes with a core of another protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// The protocols match: go on.
    Attach,
    /// An older `serve` that nothing uses: stop it and start this app's core.
    StopIdleServe,
    /// An older `serve` that is busy: ask the user before stopping it.
    AskUserThenStop,
    /// An older desktop or terminal owns the core: send `Then::Handover`. On `Declined` or
    /// after 120 s the caller fails with `an older duskcode ({core}, pid {pid}) is open; close
    /// it and try again`.
    AskOwner,
    /// This app is older: the user updates it; `open` is the desktop bundle to start, if any.
    UpdateApp {
        open: Option<PathBuf>,
    },
    Fail(String),
}

/// Decide from the versions of a core that proved it holds the key.
pub fn decide(
    app_version: &str,
    app_protocol: u32,
    interactive: bool,
    core: &CoreInfo,
    lock: Option<&LockInfo>,
) -> Next {
    if core.protocol == app_protocol {
        return Next::Attach;
    }
    if core_is_newer(app_version, app_protocol, core) {
        return if interactive {
            Next::UpdateApp {
                open: lock.and_then(|l| desktop_bundle(&l.exe)),
            }
        } else {
            Next::Fail(format!(
                "duskcode {} is running; this is {}; update duskcode",
                core.version, app_version
            ))
        };
    }
    match core.owner {
        OwnerKind::Serve if core.busy.clients == 0 && core.busy.running == 0 => Next::StopIdleServe,
        OwnerKind::Serve if interactive => Next::AskUserThenStop,
        OwnerKind::Serve => {
            Next::Fail("an older duskcode core is busy; stop it with duskcode stop".into())
        }
        OwnerKind::Gui | OwnerKind::Tui | OwnerKind::Unknown => Next::AskOwner,
    }
}

/// Semver of the versions first, the protocol when they are equal or unreadable.
fn core_is_newer(app_version: &str, app_protocol: u32, core: &CoreInfo) -> bool {
    let by_version = match (
        semver::Version::parse(&core.version),
        semver::Version::parse(app_version),
    ) {
        (Ok(c), Ok(a)) => c.cmp(&a),
        _ => Ordering::Equal,
    };
    by_version.then(core.protocol.cmp(&app_protocol)).is_gt()
}

/// The executable when it sits in a macOS app bundle that is still there.
fn desktop_bundle(exe: &std::path::Path) -> Option<PathBuf> {
    let bundle = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))?;
    bundle.exists().then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests;
