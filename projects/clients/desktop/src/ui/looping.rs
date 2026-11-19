//! Decorations that go round while something works: a ring turning, a dot breathing, stars
//! twinkling. The window is one view, so any frame one of them asks for draws all of it.
//! gpui's repeating animation asks for every display frame, and when capped it keeps a
//! timer of its own per element: loops that began at different moments still fill the
//! frames between them. Here every loop reads one clock and asks for the next tick of one
//! grid of `FPS`, so however many run, the window draws at most `FPS` times a second for
//! them. When the system asks for less motion a loop stands still at its start.

use std::{
    cell::RefCell,
    collections::HashMap,
    panic::Location,
    sync::OnceLock,
    time::{Duration, Instant},
};

use gpui_kit::*;

/// The ticks a second of the shared grid: a ring turning, a dot breathing or the mascot
/// read as smooth at this rate.
pub const FPS: u32 = 30;
const TICK_NANOS: u64 = 1_000_000_000 / FPS as u64;

/// The moment the grid and every loop's phase count from.
static EPOCH: OnceLock<Instant> = OnceLock::new();

thread_local! {
    /// Per view, the earliest redraw already asked for, so loops drawn in the same frame
    /// share one timer.
    static ASKED: RefCell<HashMap<EntityId, Instant>> = RefCell::default();
}

fn epoch(now: Instant) -> Instant {
    *EPOCH.get_or_init(|| now)
}

/// Where a loop of `period` stands at `now`, from 0 to 1, by the shared clock.
pub fn phase(period: Duration, now: Instant) -> f32 {
    let period = period.as_nanos().max(1);
    let into = now.saturating_duration_since(epoch(now)).as_nanos() % period;
    into as f32 / period as f32
}

/// The first tick after `now` of a grid that keeps one tick in `every`.
fn next_tick(now: Instant, every: u32) -> Instant {
    let start = epoch(now);
    let interval = u128::from(TICK_NANOS * u64::from(every.max(1)));
    let ticks = now.saturating_duration_since(start).as_nanos() / interval + 1;
    start + Duration::from_nanos((ticks * interval) as u64)
}

/// Draws the current view again on the next tick that keeps one in `every`; a redraw
/// already asked for at that tick or before it is enough.
pub fn ask_redraw(every: u32, window: &mut Window, cx: &mut App) {
    let now = cx.background_executor().now();
    let at = next_tick(now, every);
    let view = window.current_view();
    let asked = ASKED.with_borrow_mut(|asked| match asked.get(&view) {
        Some(&pending) if pending > now && pending <= at => false,
        _ => {
            asked.insert(view, at);
            true
        }
    });
    if !asked {
        return;
    }
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(at - now).await;
            ASKED.with_borrow_mut(|asked| {
                if asked.get(&view) == Some(&at) {
                    asked.remove(&view);
                }
            });
            cx.update(move |_, cx| cx.notify(view)).ok();
        })
        .detach();
}

/// An element drawn anew on each tick with the phase of its loop.
pub struct Looping<E> {
    element: Option<E>,
    period: Duration,
    every: u32,
    animator: Option<Box<dyn FnOnce(E, f32) -> AnyElement>>,
}

impl<E> Looping<E> {
    /// Draws on one tick in `every` only: for a slow drift that is costly to draw.
    pub fn every(mut self, every: u32) -> Self {
        self.every = every.max(1);
        self
    }
}

pub trait Loop: IntoElement + Sized + 'static {
    /// Goes round once every `period`: `animator` draws the element at a phase from 0 to 1.
    fn looping<R: IntoElement>(
        self,
        period: Duration,
        animator: impl FnOnce(Self, f32) -> R + 'static,
    ) -> Looping<Self> {
        Looping {
            element: Some(self),
            period,
            every: 1,
            animator: Some(Box::new(move |element, phase| {
                animator(element, phase).into_any_element()
            })),
        }
    }
}

impl<E: IntoElement + 'static> Loop for E {}

impl<E: IntoElement + 'static> IntoElement for Looping<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + 'static> Element for Looping<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let phase = if cx.reduce_motion() {
            0.
        } else {
            ask_redraw(self.every, window, cx);
            phase(self.period, cx.background_executor().now())
        };
        let element = self.element.take().expect("laid out once");
        let animator = self.animator.take().expect("laid out once");
        let mut element = animator(element, phase);
        (element.request_layout(window, cx), element)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{TICK_NANOS, epoch, next_tick, phase};

    #[test]
    fn loops_share_ticks_on_one_grid() {
        let start = epoch(Instant::now());
        let tick = Duration::from_nanos(TICK_NANOS);
        // Asked at any moment within a tick, the redraw falls on the tick's end.
        for offset in [Duration::ZERO, tick / 3, tick - Duration::from_nanos(1)] {
            assert_eq!(next_tick(start + tick * 7 + offset, 1), start + tick * 8);
        }
        // A slower loop's ticks are ticks of the grid too, so they add no frames.
        assert_eq!(next_tick(start + tick * 7, 2), start + tick * 8);
        assert_eq!(next_tick(start + tick * 8, 2), start + tick * 10);
    }

    #[test]
    fn a_phase_goes_round_with_its_period() {
        let start = epoch(Instant::now());
        let period = Duration::from_millis(900);
        assert_eq!(phase(period, start), 0.);
        assert!((phase(period, start + period / 4) - 0.25).abs() < 1e-6);
        assert!((phase(period, start + period * 3 + period / 2) - 0.5).abs() < 1e-6);
    }
}
