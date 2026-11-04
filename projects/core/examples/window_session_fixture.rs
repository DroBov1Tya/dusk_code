//! Disposable AppKit target/cover for the ignored window-session test.

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    fixture::run();
}

#[cfg(target_os = "macos")]
mod fixture {
    use std::{
        cell::Cell,
        io::{self, BufRead, Write},
        sync::mpsc,
        time::{Duration, Instant},
    };

    use core_graphics::event::CGEvent;
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel,
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSColor,
        NSScreen, NSWindow, NSWindowStyleMask, NSWorkspace,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{NSDate, NSObject, NSRunLoop, NSString};
    use serde_json::json;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. The counter is main-thread-only.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = Cell<usize>]
        struct ClickCounter;

        impl ClickCounter {
            #[unsafe(method(pressed:))]
            fn pressed(&self, _sender: &NSButton) {
                self.ivars().set(self.ivars().get() + 1);
            }
        }
    );

    impl ClickCounter {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let counter = Self::alloc(mtm).set_ivars(Cell::new(0));
            // SAFETY: NSObject's initializer is valid for this allocated subclass.
            unsafe { msg_send![super(counter), init] }
        }
    }

    fn button_frame() -> CGRect {
        CGRect::new(CGPoint::new(50.0, 70.0), CGSize::new(180.0, 50.0))
    }

    fn make_window(
        mtm: MainThreadMarker,
        title: &str,
        x: f64,
    ) -> (Retained<NSWindow>, Retained<ClickCounter>) {
        // SAFETY: the fixture creates and uses every AppKit object on its main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::new(CGPoint::new(x, 100.0), CGSize::new(380.0, 240.0)),
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Resizable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the fixture owns the window even after the close command.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        let counter = ClickCounter::new(mtm);
        let title_string = NSString::from_str("Fixture control");
        // SAFETY: the retained counter handles pressed: from an NSButton.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &title_string,
                Some(&counter),
                Some(sel!(pressed:)),
                mtm,
            )
        };
        button.setFrame(button_frame());
        let color = if title.ends_with("cover") {
            NSColor::redColor()
        } else {
            NSColor::greenColor()
        };
        window.setBackgroundColor(Some(&color));
        window.contentView().unwrap().addSubview(&button);
        (window, counter)
    }

    pub fn run() {
        let mode = std::env::args().nth(1).expect("target or cover");
        assert!(matches!(mode.as_str(), "target" | "cover"));
        let mtm = MainThreadMarker::new().unwrap();
        let app = NSApplication::sharedApplication(mtm);
        let activate = std::env::var_os("DUSK_WINDOW_NO_ACTIVATE").is_none();
        app.setActivationPolicy(if activate {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        });
        app.finishLaunching();
        let (window, counter) = make_window(mtm, &format!("Dusk scoped {mode}"), 100.0);
        let (sibling, sibling_counter) = make_window(mtm, "Dusk scoped sibling", 110.0);
        let height = NSScreen::screens(mtm)
            .firstObject()
            .unwrap()
            .frame()
            .size
            .height;
        if activate {
            window.makeKeyAndOrderFront(None);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
        window.orderFrontRegardless();

        let snapshot = |event: &str| {
            let rect = window.convertRectToScreen(button_frame());
            let pointer = CGEvent::new(
                core_graphics::event_source::CGEventSource::new(
                    core_graphics::event_source::CGEventSourceStateID::CombinedSessionState,
                )
                .unwrap(),
            )
            .unwrap()
            .location();
            let payload = json!({
                "event": event,
                "pid": std::process::id(),
                "window_id": window.windowNumber(),
                "button_x": rect.origin.x + rect.size.width / 2.0,
                "button_y": height - rect.origin.y - rect.size.height / 2.0,
                "front_pid": NSWorkspace::sharedWorkspace().frontmostApplication()
                    .map(|app| app.processIdentifier()),
                "minimized": window.isMiniaturized(),
                "pointer_x": pointer.x,
                "pointer_y": pointer.y,
                "state": counter.ivars().get(),
                "sibling_state": sibling_counter.ivars().get(),
            });
            println!("{payload}");
            io::stdout().flush().unwrap();
        };

        let (commands, received) = mpsc::channel();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                if commands.send(line).is_err() {
                    break;
                }
            }
        });
        let run_loop = NSRunLoop::currentRunLoop();
        let started = Instant::now();
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.5));
        snapshot("ready");
        while started.elapsed() < Duration::from_secs(45) {
            run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
            match received.try_recv() {
                Ok(command) => {
                    match command.as_str() {
                        "snapshot" => {}
                        "sibling" => sibling.orderFrontRegardless(),
                        "unsibling" => sibling.orderOut(None),
                        "move" => window.setFrameOrigin(CGPoint::new(150.0, 120.0)),
                        "resize" => window.setContentSize(CGSize::new(400.0, 250.0)),
                        "title" => window.setTitle(&NSString::from_str("Renamed target")),
                        "minimize" => window.miniaturize(None),
                        "restore" => window.deminiaturize(None),
                        "close" => window.close(),
                        _ => panic!("invalid fixture command"),
                    }
                    run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.15));
                    snapshot(&command);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
    }
}
