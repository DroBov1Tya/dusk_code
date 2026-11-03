//! The helper side of the cursor: a borderless window above every other, which the mouse
//! passes through and screenshots leave out. It holds an arrow like the system's, dark
//! with a glowing amber rim, that rocks a little about its tip. Core Animation runs the
//! rocking, so the loop here only moves the window.

use std::{
    io::BufRead,
    sync::mpsc::{self, TryRecvError},
};

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen,
    NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowSharingType,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_foundation::{NSDate, NSNumber, NSRunLoop, NSString};
use objc2_quartz_core::{
    CABasicAnimation, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer,
    kCAMediaTimingFunctionEaseInEaseOut,
};

/// The window around the arrow, wide enough for its glow and its swing.
const SIZE: CGFloat = 56.0;
/// From the window's top left corner to the arrow's tip.
const PAD: CGFloat = 14.0;
/// The arrow with its tip at the origin, y down, in points: the classic pointer shape.
const ARROW: [(CGFloat, CGFloat); 7] = [
    (0.0, 0.0),
    (0.0, 19.0),
    (4.6, 14.8),
    (7.8, 22.0),
    (10.6, 20.8),
    (7.5, 13.8),
    (13.6, 13.8),
];
/// The swing either way, in radians (about four degrees), and one way's duration.
const SWING: f64 = 0.07;
const SWING_SECONDS: f64 = 0.9;
/// How often the loop looks for a new position.
const TICK_SECONDS: f64 = 1.0 / 30.0;

/// In the cursor's helper process, draws the cursor until its parent goes and returns
/// true; otherwise returns false at once. A client calls it first thing in `main`, on
/// the main thread, which AppKit needs.
pub fn run_overlay_if_asked() -> bool {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == super::cursor::OVERLAY_ARG)
    {
        run();
        return true;
    }
    false
}

/// Runs until stdin closes: each line is "x y", the tip's place in display points with
/// the origin at the top left of the main display.
pub fn run() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let window = window(mtm);
    let (positions, received) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Some(at) = parse(&line)
                && positions.send(at).is_err()
            {
                break;
            }
        }
    });
    loop {
        let mut latest = None;
        loop {
            match received.try_recv() {
                Ok(at) => latest = Some(at),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if let Some((x, y)) = latest {
            // AppKit counts from the bottom of the main display.
            let height = NSScreen::screens(mtm)
                .firstObject()
                .map_or(0.0, |screen| screen.frame().size.height);
            window.setFrameOrigin(CGPoint::new(x - PAD, height - y - (SIZE - PAD)));
            window.orderFrontRegardless();
        }
        NSRunLoop::currentRunLoop()
            .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(TICK_SECONDS));
    }
}

fn parse(line: &str) -> Option<(CGFloat, CGFloat)> {
    let mut numbers = line.split_whitespace().map(str::parse::<CGFloat>);
    match (numbers.next()?, numbers.next()?, numbers.next()) {
        (Ok(x), Ok(y), None) if x.is_finite() && y.is_finite() => Some((x, y)),
        _ => None,
    }
}

fn window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIZE, SIZE));
    // SAFETY: a plain borderless window, made and used on the main thread.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the Rust side owns the window and keeps it for the whole run.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setIgnoresMouseEvents(true);
    window.setHasShadow(false);
    window.setLevel(NSScreenSaverWindowLevel);
    window.setSharingType(NSWindowSharingType::None);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary,
    );
    if let Some(view) = window.contentView() {
        view.setWantsLayer(true);
        if let Some(root) = view.layer() {
            root.addSublayer(&arrow());
        }
    }
    window
}

/// The arrow layer, anchored at its tip so the rocking turns about the point it marks.
fn arrow() -> Retained<CAShapeLayer> {
    let path = CGMutablePath::new();
    let [(x, y), rest @ ..] = ARROW;
    // The layer is y up: the tip sits PAD below the top.
    let point = |x: CGFloat, y: CGFloat| (PAD + x, SIZE - PAD - y);
    // SAFETY: a null transform means none; the path is ours.
    unsafe {
        let (x, y) = point(x, y);
        CGMutablePath::move_to_point(Some(&path), std::ptr::null(), x, y);
        for (x, y) in rest {
            let (x, y) = point(x, y);
            CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), x, y);
        }
    }
    CGMutablePath::close_subpath(Some(&path));
    let layer = CAShapeLayer::new();
    layer.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIZE, SIZE)));
    layer.setAnchorPoint(CGPoint::new(PAD / SIZE, (SIZE - PAD) / SIZE));
    layer.setPosition(CGPoint::new(PAD, SIZE - PAD));
    layer.setPath(Some(&path));
    layer.setFillColor(Some(&CGColor::new_srgb(0.09, 0.07, 0.14, 0.94)));
    layer.setStrokeColor(Some(&CGColor::new_srgb(1.0, 0.74, 0.34, 1.0)));
    layer.setLineWidth(1.6);
    // The glow: an amber shadow straight under the rim.
    layer.setShadowColor(Some(&CGColor::new_srgb(1.0, 0.66, 0.24, 1.0)));
    layer.setShadowOpacity(0.9);
    layer.setShadowRadius(5.0);
    layer.setShadowOffset(CGSize::new(0.0, 0.0));
    layer.addAnimation_forKey(&rocking(), Some(&NSString::from_str("rock")));
    layer
}

fn rocking() -> Retained<CABasicAnimation> {
    let rocking =
        CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.rotation.z")));
    // SAFETY: rotation takes numbers.
    unsafe {
        rocking.setFromValue(Some(&NSNumber::numberWithDouble(-SWING)));
        rocking.setToValue(Some(&NSNumber::numberWithDouble(SWING)));
    }
    rocking.setDuration(SWING_SECONDS);
    rocking.setAutoreverses(true);
    rocking.setRepeatCount(f32::INFINITY);
    // SAFETY: a named function from Core Animation's own list.
    let ease =
        CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseInEaseOut });
    rocking.setTimingFunction(Some(&ease));
    rocking
}
