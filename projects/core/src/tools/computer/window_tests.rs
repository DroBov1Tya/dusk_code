//! Opt-in integration tests with two disposable AppKit fixtures.
//! Build with `cargo build -p dusk_core --example window_session_fixture`.
//! Set `DUSK_WINDOW_FIXTURE` to that binary, then run the ignored tests serially.
//! The default fixture briefly activates its own windows. Set `DUSK_WINDOW_NO_ACTIVATE`
//! to exercise accessory windows which were never explicitly activated.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use super::super::{backend::NativeWindow, click::PointerMode};
use super::*;
use serde_json::Value;

struct Fixture {
    process: Child,
    events: mpsc::Receiver<Value>,
}

impl Fixture {
    fn start(mode: &str) -> (Self, Value) {
        let binary = std::env::var("DUSK_WINDOW_FIXTURE")
            .expect("build the window_session_fixture example and set DUSK_WINDOW_FIXTURE");
        let mut process = Command::new(binary)
            .arg(mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = process.stdout.take().unwrap();
        let (sender, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Ok(value) = serde_json::from_str(&line) {
                    let _ = sender.send(value);
                }
            }
        });
        let fixture = Self { process, events };
        let ready = fixture.event("ready");
        (fixture, ready)
    }

    fn event(&self, event: &str) -> Value {
        for _ in 0..10 {
            let value = self
                .events
                .recv_timeout(Duration::from_secs(5))
                .expect("fixture stopped responding");
            if value["event"] == event {
                return value;
            }
        }
        panic!("missing fixture event {event}");
    }

    fn command(&mut self, command: &str) -> Value {
        writeln!(self.process.stdin.as_mut().unwrap(), "{command}").unwrap();
        self.event(command)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn seed_view(screen: &Screen, window: &Window) {
    *screen.view.lock() = Some(View {
        origin: Point {
            x: window.bounds.x,
            y: window.bounds.y,
        },
        scale_x: 1.0,
        scale_y: 1.0,
        width: window.bounds.width,
        height: window.bounds.height,
        window: Some(window.into()),
    });
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

fn assert_accessibility() {
    // SAFETY: this function queries permission without requesting it.
    assert_ne!(
        unsafe { AXIsProcessTrusted() },
        0,
        "Accessibility preflight denied"
    );
}

fn assert_green(screen: &Screen, card: &Card, x: u32, y: u32) {
    let pixels = image::load_from_memory(&screen.artifacts.read(card).unwrap())
        .unwrap()
        .into_rgb8();
    let [r, g, b] = pixels.get_pixel(x, y).0;
    assert!(
        g > r.saturating_add(50) && g > b.saturating_add(50),
        "expected target's green pixels at ({x}, {y}), got ({r}, {g}, {b})"
    );
}

#[test]
#[ignore = "requires Screen Recording, Accessibility and a built window_session_fixture example"]
fn window_capture_and_regions_exclude_covering_window() {
    // SAFETY: this function queries permission without requesting it.
    assert!(
        unsafe { CGPreflightScreenCaptureAccess() },
        "Screen Recording preflight denied"
    );
    assert_accessibility();
    let (mut target, ready) = Fixture::start("target");
    let id = ready["window_id"].as_u64().unwrap() as u32;
    let pid = ready["pid"].as_i64().unwrap() as i32;
    let session = NativeWindow::select(id, pid).unwrap();
    let window = session.inspect().unwrap();
    let token = session.token.clone();
    let artifacts_dir = std::env::temp_dir().join(format!("dusk-window-test-{}", Uuid::new_v4()));
    let screen = Screen::new(ArtifactStore::new(artifacts_dir.clone()));
    let (mut cover, cover_ready) = Fixture::start("cover");
    let cover_id = cover_ready["window_id"].as_u64().unwrap() as u32;
    let fixtures: Vec<_> = Desktop
        .windows()
        .unwrap()
        .into_iter()
        .filter(|w| w.id == id || w.id == cover_id)
        .collect();
    assert_eq!(fixtures.len(), 2);
    assert_eq!(fixtures[0].id, cover_id, "cover must be in front");
    assert_eq!(
        fixtures[0].bounds, window.bounds,
        "cover must overlap target"
    );
    let crop = || {
        Target::Region(Region {
            x: 5.0,
            y: 55.0,
            width: 20.0,
            height: 20.0,
        })
    };
    let (named, _) = screen
        .capture(Target::Window("Dusk scoped target".into()), None)
        .unwrap();
    assert_eq!(named.width, window.bounds.width as u32);
    assert_eq!(named.height, window.bounds.height as u32);
    assert_green(&screen, &named, 15, 65);
    let (named_crop, _) = screen.capture(crop(), None).unwrap();
    assert_green(&screen, &named_crop, 10, 10);
    let (nested, _) = screen
        .capture(
            Target::Region(Region {
                x: 2.0,
                y: 2.0,
                width: 10.0,
                height: 10.0,
            }),
            None,
        )
        .unwrap();
    assert_green(&screen, &nested, 5, 5);
    target.command("move");
    let error = screen
        .capture(
            Target::Region(Region {
                x: 2.0,
                y: 2.0,
                width: 4.0,
                height: 4.0,
            }),
            None,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("captured window closed, moved or resized")
    );
    assert!(
        screen.view().is_err(),
        "failed capture must clear coordinates"
    );
    target.command("reset");

    screen.select(session, None).unwrap();
    let (selected, _) = screen.capture(Target::Display, Some(&token)).unwrap();
    assert_green(&screen, &selected, 15, 65);
    let (selected_crop, _) = screen.capture(crop(), Some(&token)).unwrap();
    assert_green(&screen, &selected_crop, 10, 10);

    screen.capture(Target::Display, Some(&token)).unwrap();
    let x = ready["button_x"].as_f64().unwrap() - window.bounds.x;
    let y = ready["button_y"].as_f64().unwrap() - window.bounds.y;
    let (button_crop, _) = screen
        .capture(
            Target::Region(Region {
                x: x - 20.0,
                y: y - 20.0,
                width: 40.0,
                height: 40.0,
            }),
            Some(&token),
        )
        .unwrap();
    assert_eq!((button_crop.width, button_crop.height), (40, 40));
    let before = cover.command("snapshot");
    screen
        .click(20.0, 20.0, false, 1, PointerMode::Independent, Some(&token))
        .unwrap();
    let after = cover.command("snapshot");
    let target_state = target.command("snapshot");
    assert_eq!(target_state["state"], 1);
    assert_eq!(target_state["sibling_state"], 0);
    assert_eq!(after["state"], 0);
    assert_eq!(after["pointer_x"], before["pointer_x"]);
    assert_eq!(after["pointer_y"], before["pointer_y"]);
    target.command("close");
    assert!(screen.capture(crop(), Some(&token)).is_err());
    assert!(screen.view().is_err());
    let _ = std::fs::remove_dir_all(&artifacts_dir);
}

#[test]
#[ignore = "requires Accessibility and a built window_session_fixture example"]
fn covered_window_session_preserves_pointer_focus_and_rejects_siblings_and_stale_views() {
    assert_accessibility();
    let (mut target, ready) = Fixture::start("target");
    let id = ready["window_id"].as_u64().unwrap() as u32;
    let pid = ready["pid"].as_i64().unwrap() as i32;
    let session = NativeWindow::select(id, pid).unwrap_or_else(|error| {
        let listed: Vec<_> = Desktop
            .windows()
            .unwrap()
            .into_iter()
            .filter(|window| window.pid == pid)
            .map(|window| window.id)
            .collect();
        panic!("fixture target {id} not selectable: {error:#}; listed own windows {listed:?}");
    });
    let initial = session.inspect().unwrap();
    let token = session.token.clone();
    let artifacts_dir = std::env::temp_dir().join(format!("dusk-window-test-{}", Uuid::new_v4()));
    let screen = Screen::new(ArtifactStore::new(artifacts_dir.clone()));
    let another_chat = Screen::new(ArtifactStore::new(artifacts_dir.clone()));
    screen.select(session.clone(), None).unwrap();
    assert!(screen.require_unscoped().is_err());
    assert!(another_chat.require_unscoped().is_ok());
    assert!(
        screen
            .unscoped::<()>(|| panic!("shared input escaped confinement"))
            .is_err()
    );
    assert!(
        screen
            .click(0.0, 0.0, false, 1, PointerMode::Independent, None)
            .is_err()
    );
    assert!(
        screen
            .click(0.0, 0.0, false, 1, PointerMode::System, Some(&token))
            .is_err()
    );
    assert!(
        screen
            .click(0.0, 0.0, false, 1, PointerMode::Independent, Some(&token))
            .is_err()
    );
    let (mut cover, cover_ready) = Fixture::start("cover");
    let cover_id = cover_ready["window_id"].as_u64().unwrap() as u32;
    let fixtures: Vec<_> = Desktop
        .windows()
        .unwrap()
        .into_iter()
        .filter(|w| w.id == initial.id || w.id == cover_id)
        .collect();
    assert_eq!(fixtures.len(), 2);
    assert_eq!(fixtures[0].id, cover_id, "cover must be in front");
    assert_eq!(
        fixtures[0].bounds, initial.bounds,
        "cover must overlap target"
    );
    seed_view(&screen, &initial);
    let before = cover.command("snapshot");
    let x = ready["button_x"].as_f64().unwrap() - initial.bounds.x;
    let y = ready["button_y"].as_f64().unwrap() - initial.bounds.y;
    screen
        .click(x, y, false, 1, PointerMode::Independent, Some(&token))
        .unwrap();
    let after_press = cover.command("snapshot");
    assert_eq!(after_press["state"], 0);
    assert_eq!(target.command("snapshot")["state"], 1);
    let after_callback = cover.command("snapshot");
    target.command("sibling");
    let error = screen
        .click(x, y, false, 1, PointerMode::Independent, Some(&token))
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("outside the selected window"),
        "{error:#}"
    );
    let states = target.command("snapshot");
    assert_eq!(states["state"], 1);
    assert_eq!(states["sibling_state"], 0);
    target.command("unsibling");
    target.command("title");
    assert!(
        session.inspect().is_ok(),
        "retained identity survives title changes"
    );
    target.command("move");
    assert!(
        screen
            .click(x, y, false, 1, PointerMode::Independent, Some(&token))
            .is_err()
    );
    let moved = session.inspect().unwrap();
    assert_ne!(moved.bounds, initial.bounds);
    seed_view(&screen, &moved);
    target.command("resize");
    assert!(
        screen
            .click(x, y, false, 1, PointerMode::Independent, Some(&token))
            .is_err()
    );
    assert!(
        screen
            .capture(Target::Window("cover".into()), Some(&token))
            .is_err()
    );
    assert!(
        screen.view().is_err(),
        "failed capture clears stale coordinates"
    );
    let minimized = target.command("minimize");
    assert_eq!(minimized["minimized"], true);
    assert!(session.inspect().is_err());
    target.command("restore");
    assert!(session.inspect().is_ok());
    target.command("close");
    assert!(session.inspect().is_err());
    assert!(NativeWindow::select(initial.id, initial.pid).is_err());
    assert!(screen.release("stale-token").is_err());
    assert!(screen.require_unscoped().is_err());
    screen.release(&token).unwrap();
    assert!(screen.require_unscoped().is_ok());
    assert!(screen.view().is_err());
    let _ = std::fs::remove_dir_all(&artifacts_dir);
    println!(
        "PASS covered callback, sibling rejection, move/resize/minimize/close rejection, scope guards and release"
    );
    assert!(before["front_pid"].is_number(), "frontmost PID unavailable");
    if std::env::var_os("DUSK_WINDOW_NO_ACTIVATE").is_none() {
        assert_eq!(
            before["front_pid"], cover_ready["pid"],
            "cover was not foreground"
        );
    }
    assert_ne!(
        before["front_pid"], ready["pid"],
        "target was already foreground"
    );
    for after in [after_press, after_callback] {
        assert_ne!(
            after["front_pid"], ready["pid"],
            "AXPress activated the target"
        );
        assert!(
            before["front_pid"] == after["front_pid"],
            "foreground changed between non-target apps (cover before={}, after={})",
            before["front_pid"] == cover_ready["pid"],
            after["front_pid"] == cover_ready["pid"]
        );
        assert_eq!(before["pointer_x"], after["pointer_x"]);
        assert_eq!(before["pointer_y"], after["pointer_y"]);
    }
    println!("PASS actual Rust AXPress: focus and pointer preserved.");
}
