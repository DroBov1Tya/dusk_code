//! A sideways swipe on a trackpad. gpui has no swipe gesture for it, but a trackpad's
//! scroll events carry exact pixels and, on macOS, the phase of the touch: the sideways
//! distance of one touch is added up, and once it is far enough the page turns, once a
//! touch. The momentum after the fingers lift still belongs to that touch. A mouse wheel
//! gives lines, not pixels, and turns nothing.

use std::time::{Duration, Instant};

use gpui_kit::*;

/// How far the fingers go sideways before the page turns.
const FAR: f32 = 60.;
/// Where the platform tells no phase, a pause this long starts a new touch.
const PAUSE: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct Swipe {
    /// The sideways and the upright distance of the touch so far.
    x: f32,
    y: f32,
    last: Option<Instant>,
    /// The touch already turned a page.
    spent: bool,
}

impl Swipe {
    /// The page step a scroll event completes: `1` for the next page, `-1` for the one
    /// before, `0` for none yet.
    pub fn feed(&mut self, event: &ScrollEvent) -> isize {
        let ScrollDelta::Pixels(delta) = event.delta else {
            return 0;
        };
        let now = Instant::now();
        let paused = self.last.is_none_or(|last| now - last > PAUSE);
        if event.touch_phase == TouchPhase::Started || paused {
            *self = Self::default();
        }
        self.last = Some(now);
        if self.spent {
            return 0;
        }
        self.x += delta.x.as_f32();
        self.y += delta.y.as_f32();
        // Mostly sideways, so reading down a page never turns it.
        if self.x.abs() < FAR || self.x.abs() < self.y.abs() * 2. {
            return 0;
        }
        self.spent = true;
        // Content follows the fingers: moving it left brings the next page.
        if self.x < 0. { 1 } else { -1 }
    }
}
