use std::path::PathBuf;

use dusk_protocol::client::{Busy, CoreInfo, OwnerKind};

use super::*;

const OWNERS: [OwnerKind; 3] = [OwnerKind::Gui, OwnerKind::Tui, OwnerKind::Serve];

fn core(version: &str, protocol: u32, owner: OwnerKind, clients: u32, running: u32) -> CoreInfo {
    CoreInfo {
        protocol,
        oldest: 1,
        version: version.into(),
        owner,
        busy: Busy { clients, running },
    }
}

fn lock(exe: PathBuf) -> LockInfo {
    LockInfo {
        pid: 7,
        version: "9.0.0".into(),
        protocol: 9,
        owner: OwnerKind::Gui,
        started_at: 0,
        exe,
    }
}

#[test]
fn the_same_protocol_attaches() {
    let c = core("0.70.0", 3, OwnerKind::Gui, 2, 1);
    assert_eq!(decide("0.77.0", 3, true, &c, None), Next::Attach);
}

#[test]
fn an_older_app_never_stops_the_core() {
    for owner in OWNERS {
        for (clients, running) in [(0, 0), (1, 0), (0, 1), (3, 2)] {
            for interactive in [true, false] {
                let c = core("0.80.0", 4, owner, clients, running);
                match decide("0.77.0", 3, interactive, &c, None) {
                    Next::UpdateApp { .. } if interactive => {}
                    Next::Fail(m) if !interactive => {
                        assert_eq!(
                            m,
                            "duskcode 0.80.0 is running; this is 0.77.0; update duskcode"
                        );
                    }
                    other => panic!("{owner:?} {clients}/{running} {interactive}: {other:?}"),
                }
            }
        }
    }
}

#[test]
fn the_protocol_breaks_a_tie_of_versions() {
    let c = core("0.77.0", 4, OwnerKind::Serve, 0, 0);
    assert!(matches!(
        decide("0.77.0", 3, true, &c, None),
        Next::UpdateApp { open: None }
    ));
    let c = core("0.77.0", 2, OwnerKind::Serve, 0, 0);
    assert_eq!(decide("0.77.0", 3, true, &c, None), Next::StopIdleServe);
}

#[test]
fn a_desktop_bundle_that_exists_is_offered() {
    let dir = std::env::temp_dir().join(format!("dusk-version-{}", std::process::id()));
    let exe = dir.join("Dusk.app/Contents/MacOS/dusk");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    let c = core("0.80.0", 4, OwnerKind::Gui, 0, 0);
    let got = decide("0.77.0", 3, true, &c, Some(&lock(exe)));
    assert_eq!(
        got,
        Next::UpdateApp {
            open: Some(dir.join("Dusk.app"))
        }
    );
    std::fs::remove_dir_all(&dir).unwrap();
    let gone = lock(dir.join("Dusk.app/Contents/MacOS/dusk"));
    assert_eq!(
        decide("0.77.0", 3, true, &c, Some(&gone)),
        Next::UpdateApp { open: None }
    );
    let plain = lock(PathBuf::from("/usr/bin/duskcode"));
    assert_eq!(
        decide("0.77.0", 3, true, &c, Some(&plain)),
        Next::UpdateApp { open: None }
    );
}

#[test]
fn an_idle_older_serve_is_stopped() {
    let c = core("0.70.0", 2, OwnerKind::Serve, 0, 0);
    for interactive in [true, false] {
        assert_eq!(
            decide("0.77.0", 3, interactive, &c, None),
            Next::StopIdleServe
        );
    }
}

#[test]
fn a_busy_older_serve_asks_or_fails() {
    for (clients, running) in [(1, 0), (0, 1), (2, 2)] {
        let c = core("0.70.0", 2, OwnerKind::Serve, clients, running);
        assert_eq!(decide("0.77.0", 3, true, &c, None), Next::AskUserThenStop);
        assert_eq!(
            decide("0.77.0", 3, false, &c, None),
            Next::Fail("an older duskcode core is busy; stop it with duskcode stop".into())
        );
    }
}

#[test]
fn a_gui_owner_is_always_asked() {
    for owner in [OwnerKind::Gui, OwnerKind::Tui] {
        for (clients, running) in [(0, 0), (1, 0), (2, 3)] {
            for interactive in [true, false] {
                let c = core("0.70.0", 2, owner, clients, running);
                assert_eq!(decide("0.77.0", 3, interactive, &c, None), Next::AskOwner);
            }
        }
    }
}
