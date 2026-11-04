//! The helper side of the cursor: dusk's own arrow, drawn in a window over the whole
//! main display, which the mouse passes through and screenshots leave out. A line on
//! stdin is the tip's place in display points, and Core Animation glides it there; a
//! `t` says the agent is at work without moving the tip. After ten seconds of silence
//! the arrow fades and goes away, and the next line brings it back. The window and its
//! layers are made once; the rocking runs from the first line to the last.

use std::{
    io::BufRead,
    sync::mpsc::{self, TryRecvError},
    time::{Duration, Instant},
};

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSScreen,
    NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowSharingType,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath};
use objc2_foundation::{NSDate, NSNumber, NSRunLoop, NSString, NSValue};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer,
    kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionEaseOut,
};

/// The square the arrow is drawn in, wide enough for its glow and its swing.
const SIZE: CGFloat = 64.0;
/// From the square's bottom left to its tip, in points.
const PAD: CGFloat = 10.0;
/// A point of the arrow in model space: the tip at the origin, y down.
type Point = (CGFloat, CGFloat);
/// The arrow's seven points, each 0.82× the straight-edged arrow it replaces: like the
/// system's pointer, a shade smaller, its two lower edges bent a little inward.
const TIP: Point = (0.0, 0.0);
const LEG: Point = (0.0, 15.6);
const NOTCH: Point = (3.8, 12.1);
const LOW: Point = (6.4, 18.0);
const SIDE: Point = (8.7, 17.1);
const WRIST: Point = (6.2, 11.3);
const FLANK: Point = (11.2, 11.3);
/// The controls that bend the two lower edges.
const BEND_LEFT: Point = (2.8, 12.8);
const BEND_RIGHT: Point = (8.6, 10.0);
/// The swing either way, in radians (about four degrees), and one way's duration.
const SWING: f64 = 0.07;
const SWING_SECONDS: f64 = 0.9;
/// How often the loop looks for a line, and for the idle timeout it also guards.
const TICK_SECONDS: f64 = 1.0 / 30.0;
/// Silence this long hides the arrow; the next line brings it back.
const IDLE_SECONDS: f64 = 10.0;
/// The fades out and in.
const FADE_OUT_SECONDS: f64 = 0.30;
const FADE_IN_SECONDS: f64 = 0.15;
/// A glide's duration in milliseconds: a little per point, clamped both ways.
const TRAVEL_BASE_MS: CGFloat = 40.0;
const TRAVEL_MS_PER_POINT: CGFloat = 0.11;
const TRAVEL_MIN_MS: CGFloat = 80.0;
const TRAVEL_MAX_MS: CGFloat = 250.0;

/// How long a glide of `distance` points takes.
fn travel_ms(distance: CGFloat) -> CGFloat {
    (TRAVEL_BASE_MS + TRAVEL_MS_PER_POINT * distance).clamp(TRAVEL_MIN_MS, TRAVEL_MAX_MS)
}

/// One step of the path, the shape as data so its tests need no AppKit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Move(Point),
    Line(Point),
    Quad { cp: Point, end: Point },
    Close,
}

/// The arrow: straight edges as lines, the two lower ones as bends, and the close back
/// to the tip.
fn shape() -> Vec<Segment> {
    vec![
        Segment::Move(TIP),
        Segment::Line(LEG),
        Segment::Quad {
            cp: BEND_LEFT,
            end: NOTCH,
        },
        Segment::Line(LOW),
        Segment::Line(SIDE),
        Segment::Line(WRIST),
        Segment::Quad {
            cp: BEND_RIGHT,
            end: FLANK,
        },
        Segment::Close,
    ]
}

/// What one line of stdin asks for: put the tip at display points, or the keepalive,
/// which only says the agent is at work.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Move { x: CGFloat, y: CGFloat },
    Keepalive,
}

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
/// the origin at the top left of the main display, or "t", the keepalive.
pub fn run() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let (window, square) = window(mtm);
    let arrow = arrow();
    square.addSublayer(&arrow);
    let (commands, received) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Some(command) = parse(&line)
                && commands.send(command).is_err()
            {
                break;
            }
        }
    });
    // Where the tip is now, in the square's points; kept here so each glide knows how
    // far it goes, and so a glide after a hide starts from where the arrow last was.
    let mut at = (0.0, 0.0);
    let mut shown = false;
    let mut last_line = Instant::now();
    let mut gone_at: Option<Instant> = None;
    loop {
        let mut latest = None;
        loop {
            match received.try_recv() {
                Ok(command) => {
                    last_line = Instant::now();
                    latest = Some(command);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if let Some(command) = latest {
            gone_at = None;
            if !shown {
                window.orderFrontRegardless();
                fade(&square, 1.0, FADE_IN_SECONDS);
                square.setOpacity(1.0);
                shown = true;
            }
            if let Command::Move { x, y } = command {
                // AppKit counts from the bottom of the main display.
                let height = NSScreen::screens(mtm)
                    .firstObject()
                    .map_or(0.0, |screen| screen.frame().size.height);
                let to = (x, height - y);
                let distance = (to.0 - at.0).hypot(to.1 - at.1);
                glide(&arrow, at, to, travel_ms(distance));
                at = to;
            }
        }
        if shown
            && gone_at.is_none()
            && last_line.elapsed() >= Duration::from_secs_f64(IDLE_SECONDS)
        {
            fade(&square, 0.0, FADE_OUT_SECONDS);
            square.setOpacity(0.0);
            gone_at = Some(Instant::now() + Duration::from_secs_f64(FADE_OUT_SECONDS));
        }
        if shown && gone_at.is_some_and(|gone| Instant::now() >= gone) {
            window.orderOut(None);
            gone_at = None;
            shown = false;
        }
        NSRunLoop::currentRunLoop()
            .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(TICK_SECONDS));
    }
}

fn parse(line: &str) -> Option<Command> {
    if line == "t" {
        return Some(Command::Keepalive);
    }
    let mut numbers = line.split_whitespace().map(str::parse::<CGFloat>);
    match (numbers.next()?, numbers.next()?, numbers.next()) {
        (Ok(x), Ok(y), None) if x.is_finite() && y.is_finite() => Some(Command::Move { x, y }),
        _ => None,
    }
}

/// The window over the whole main display, and the square the arrow hangs on: made
/// once, ordered in and out as the cursor shows and hides.
fn window(mtm: MainThreadMarker) -> (Retained<NSWindow>, Retained<CALayer>) {
    let frame = NSScreen::screens(mtm).firstObject().map_or(
        CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIZE, SIZE)),
        |screen| screen.frame(),
    );
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
    let square = CALayer::new();
    square.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIZE, SIZE)));
    if let Some(view) = window.contentView() {
        view.setWantsLayer(true);
        view.setLayer(Some(&square));
    }
    (window, square)
}

/// The arrow layer, anchored at its tip so the rocking turns about the point it marks
/// and the glide moves that point.
fn arrow() -> Retained<CAShapeLayer> {
    let path = CGMutablePath::new();
    // The layer is y up: the tip sits PAD up from its bottom.
    let point = |x: CGFloat, y: CGFloat| (PAD + x, SIZE - PAD - y);
    // SAFETY: a null transform means none; the path is ours.
    unsafe {
        for segment in shape() {
            match segment {
                Segment::Move((x, y)) => {
                    let (x, y) = point(x, y);
                    CGMutablePath::move_to_point(Some(&path), std::ptr::null(), x, y);
                }
                Segment::Line((x, y)) => {
                    let (x, y) = point(x, y);
                    CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), x, y);
                }
                Segment::Quad {
                    cp: (cx, cy),
                    end: (x, y),
                } => {
                    let (cx, cy) = point(cx, cy);
                    let (x, y) = point(x, y);
                    CGMutablePath::add_quad_curve_to_point(
                        Some(&path),
                        std::ptr::null(),
                        cx,
                        cy,
                        x,
                        y,
                    );
                }
                Segment::Close => CGMutablePath::close_subpath(Some(&path)),
            }
        }
    }
    let layer = CAShapeLayer::new();
    layer.setBounds(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIZE, SIZE)));
    layer.setAnchorPoint(CGPoint::new(PAD / SIZE, (SIZE - PAD) / SIZE));
    layer.setPosition(CGPoint::new(0.0, 0.0));
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

/// Glides the layer's anchor from `from` to `to`, points in the square: the model keeps
/// the new place, so the animation only draws the way there and the rocking is left be.
fn glide(arrow: &CAShapeLayer, from: Point, to: Point, ms: CGFloat) {
    arrow.setPosition(CGPoint::new(to.0, to.1));
    // SAFETY: a point boxed as a value, for the key path that takes one.
    unsafe {
        let from = NSValue::valueWithPoint(CGPoint::new(from.0, from.1));
        let to = NSValue::valueWithPoint(CGPoint::new(to.0, to.1));
        animate(arrow, "position", &from, &to, ms / 1000.0);
    }
}

/// Fades the square, the arrow in it, to `to`.
fn fade(square: &CALayer, to: f32, seconds: f64) {
    let from = NSNumber::numberWithDouble(f64::from(square.opacity()));
    let until = NSNumber::numberWithDouble(f64::from(to));
    animate(square, "opacity", &from, &until, seconds);
    square.setOpacity(to);
}

/// Starts one ease-out animation on the layer; the key paths take objects, so the
/// caller boxes the values.
fn animate(layer: &CALayer, key_path: &str, from: &AnyObject, to: &AnyObject, seconds: f64) {
    let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(key_path)));
    // SAFETY: the values fit the key path the caller named.
    unsafe {
        animation.setFromValue(Some(from));
        animation.setToValue(Some(to));
    }
    animation.setDuration(seconds);
    // SAFETY: a named function from Core Animation's own list.
    let ease = CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseOut });
    animation.setTimingFunction(Some(&ease));
    layer.addAnimation_forKey(&animation, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The old arrow's seven points, to keep the new one an exact 0.82 of it.
    const OLD: [Point; 7] = [
        (0.0, 0.0),
        (0.0, 19.0),
        (4.6, 14.8),
        (7.8, 22.0),
        (10.6, 20.8),
        (7.5, 13.8),
        (13.6, 13.8),
    ];

    /// The points the pen rests on, in order, the close returning to the tip.
    fn on_curve(shape: &[Segment]) -> Vec<Point> {
        let Segment::Move(first) = shape[0] else {
            panic!("the shape starts at the tip");
        };
        shape
            .iter()
            .map(|segment| match *segment {
                Segment::Move(p) | Segment::Line(p) | Segment::Quad { end: p, .. } => p,
                Segment::Close => first,
            })
            .collect()
    }

    /// The turn from a bend's chord to its control point: which side of the chord the
    /// control sits on.
    fn side(start: Point, cp: Point, end: Point) -> CGFloat {
        (cp.0 - start.0) * (end.1 - cp.1) - (cp.1 - start.1) * (end.0 - cp.0)
    }

    #[test]
    fn the_shape_is_one_closed_path_of_the_parts_asked_for() {
        let shape = shape();
        let count = |what: fn(&Segment) -> bool| shape.iter().filter(|s| what(s)).count();
        // The calls the path spec lists: one move, four lines, two bends, the close.
        assert_eq!(count(|s| matches!(s, Segment::Move(_))), 1);
        assert_eq!(count(|s| matches!(s, Segment::Line(_))), 4);
        assert_eq!(count(|s| matches!(s, Segment::Quad { .. })), 2);
        assert_eq!(count(|s| matches!(s, Segment::Close)), 1);
    }

    #[test]
    fn the_path_starts_and_ends_at_the_tip() {
        let points = on_curve(&shape());
        assert_eq!((points.first(), points.last()), (Some(&TIP), Some(&TIP)));
    }

    #[test]
    fn the_arrow_is_smaller_than_the_old_one() {
        let mut points = on_curve(&shape());
        points.extend(shape().iter().filter_map(|s| match *s {
            Segment::Quad { cp, .. } => Some(cp),
            _ => None,
        }));
        let xs = || points.iter().map(|p| p.0);
        let ys = || points.iter().map(|p| p.1);
        let width = xs().fold(f64::NEG_INFINITY, f64::max) - xs().fold(f64::INFINITY, f64::min);
        let height = ys().fold(f64::NEG_INFINITY, f64::max) - ys().fold(f64::INFINITY, f64::min);
        assert!((width - 11.2).abs() <= 0.05, "width {width}");
        assert!((height - 18.0).abs() <= 0.05, "height {height}");
        assert!(width < 13.6 && height < 22.0, "the old box is 13.6 × 22");
        assert!(width >= 0.75 * 13.6 && height >= 0.75 * 22.0, "no smaller");
    }

    #[test]
    fn both_bends_bend_inward() {
        let mut start = TIP;
        let mut bends = 0;
        for segment in shape() {
            match segment {
                Segment::Move(p) | Segment::Line(p) => start = p,
                Segment::Quad { cp, end } => {
                    assert!(side(start, cp, end) > 0.0, "{start:?} bulges outward");
                    start = end;
                    bends += 1;
                }
                Segment::Close => {}
            }
        }
        assert_eq!(bends, 2);
    }

    #[test]
    fn every_point_is_the_old_one_times_0_82() {
        let points = on_curve(&shape());
        assert_eq!(points.len(), OLD.len() + 1, "the close returns to the tip");
        for (point, old) in points.iter().zip(&OLD) {
            for (got, was) in [(point.0, old.0), (point.1, old.1)] {
                let want = 0.82 * was;
                assert!((got - want).abs() <= 0.05 + 1e-9, "{point:?} vs {old:?}");
            }
        }
    }

    #[test]
    fn a_line_is_a_move_a_keepalive_or_ignored() {
        assert_eq!(
            parse("120.5 -44"),
            Some(Command::Move { x: 120.5, y: -44.0 })
        );
        assert_eq!(parse("t"), Some(Command::Keepalive));
        for ignored in ["", "T", "t t", "x y", "1 2 3", "nope"] {
            assert_eq!(parse(ignored), None, "{ignored}");
        }
    }

    #[test]
    fn the_travel_time_clamps_both_ways() {
        assert_eq!(travel_ms(0.0), 80.0, "the floor");
        assert!((travel_ms(1000.0) - 150.0).abs() < 1e-6, "the middle");
        assert_eq!(travel_ms(3000.0), 250.0, "the ceiling");
    }
}
