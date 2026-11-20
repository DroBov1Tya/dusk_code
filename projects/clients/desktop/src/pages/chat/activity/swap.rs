//! One thing taking the place of another in a row: the words beside the mark, the mark
//! itself, a turn heading's phrase. The old one fades out and rises a little, the new one
//! comes up into its place, and the room they take eases from the old width to the new,
//! so what stands after them (the clock, the counts) glides instead of stepping.
//!
//! Drawing is where a change is seen: a place is drawn with what it says now, and when
//! that differs from what it said on the last frame, the swap begins. At most one swap
//! runs in a place. What comes while one runs takes over from where it stands: the newest
//! comes in, and what was coming in goes out from the strength it had reached, so nothing
//! flashes and nothing queues. Under reduced motion the new thing simply stands there.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::*;

/// How long a swap takes.
pub const LENGTH: Duration = Duration::from_millis(200);
/// How far the old thing rises and the new one comes up, in pixels.
const TRAVEL: f32 = 3.;
/// The old thing is gone by this share of the swap and the new one starts to show a
/// little before: words that stand in the same place are never both readable at once.
const OUT_BY: f32 = 0.45;
const IN_FROM: f32 = 0.3;

/// What a place shows: the words, and whatever else the caller draws them by.
pub trait Shown: Clone + 'static {
    /// What tells two of them apart. Words that only tick, as a countdown does, keep
    /// their key and are never swapped.
    fn key(&self) -> String;
}

impl Shown for SharedString {
    fn key(&self) -> String {
        self.to_string()
    }
}

struct Place<T> {
    now: T,
    /// The width `now` takes when nothing clips it, once it was laid out.
    width: Option<f32>,
    going: Option<Going<T>>,
}

struct Going<T> {
    was: T,
    /// The width the place had when the swap began.
    width: Option<f32>,
    /// How strong `was` stood then: less than whole when it was itself still coming in.
    strength: f32,
    /// None until the new thing's width is known: the swap starts on the frame after.
    since: Option<Instant>,
}

/// The places of one chat that swap, by name. Kept behind a cell, as drawing only reads
/// the chat; shared so a drawn frame can write back the widths it measured.
pub struct Swaps<T>(Rc<RefCell<HashMap<SharedString, Place<T>>>>);

impl<T> Default for Swaps<T> {
    fn default() -> Self {
        Self(Rc::default())
    }
}

/// Where a swap stands on this frame.
pub struct Frame<T> {
    /// The old thing and how strong it still shows, while it goes.
    pub old: Option<(T, f32)>,
    /// How strong the new thing shows.
    pub new: f32,
    /// How far the old thing has risen and the new one has yet to come up, in pixels.
    pub risen: f32,
    pub below: f32,
    /// The width of the place, between the old and the new; none when it is as wide as
    /// what it shows.
    pub width: Option<f32>,
    /// A swap runs: the next frame must be drawn.
    pub running: bool,
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// How strong the new thing shows at `t` of the swap, from 0 to 1.
fn coming(t: f32) -> f32 {
    ease_out((t - IN_FROM) / (1. - IN_FROM))
}

/// The frame of a swap `t` of the way through (0 to 1), from `strength` and between the
/// two widths when both are known.
fn frame_at<T>(was: T, strength: f32, widths: Option<(f32, f32)>, t: f32) -> Frame<T> {
    let moved = ease_out(t);
    Frame {
        old: Some((was, strength * (1. - (t / OUT_BY).clamp(0., 1.)))),
        new: coming(t),
        risen: TRAVEL * moved,
        below: TRAVEL * (1. - moved),
        width: widths.map(|(from, to)| from + (to - from) * moved),
        running: true,
    }
}

impl<T: Shown> Swaps<T> {
    /// Forgets every place: a new turn begins with nothing to swap from.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    /// Says what `place` shows now and gives the frame to draw it by. `still`: the
    /// system asks for less motion, so a change simply stands.
    pub fn frame(&self, place: &SharedString, now: T, still: bool) -> Frame<T> {
        self.frame_of(place, now, still, true)
    }

    /// `sized`: the place takes the width of what it shows, so a swap waits until the
    /// new thing was laid out and eases between the two widths.
    fn frame_of(&self, place: &SharedString, now: T, still: bool, sized: bool) -> Frame<T> {
        let at = Instant::now();
        let mut places = self.0.borrow_mut();
        let standing = Frame {
            old: None,
            new: 1.,
            risen: 0.,
            below: 0.,
            width: None,
            running: false,
        };
        let Some(known) = places.get_mut(place) else {
            let fresh = Place {
                now,
                width: None,
                going: None,
            };
            places.insert(place.clone(), fresh);
            return standing;
        };
        if known.now.key() != now.key() {
            // What stood, or was coming in, goes from the strength and width it has.
            let (strength, width) = match &known.going {
                Some(going) => {
                    let t = progress(going.since, at);
                    let between = going
                        .width
                        .zip(known.width)
                        .map(|(from, to)| from + (to - from) * ease_out(t));
                    (coming(t), between.or(going.width))
                }
                None => (1., known.width),
            };
            let was = std::mem::replace(&mut known.now, now);
            known.width = None;
            known.going = (!still).then_some(Going {
                was,
                width,
                strength,
                since: None,
            });
        } else {
            // The same thing, perhaps with words that ticked.
            known.now = now;
        }
        let Some(going) = &mut known.going else {
            return standing;
        };
        if still {
            known.going = None;
            return standing;
        }
        // The swap starts once the new thing was laid out and its width is known; until
        // then the old one stands in the room it had.
        if going.since.is_none() && (known.width.is_some() || !sized) {
            going.since = Some(at);
        }
        let t = progress(going.since, at);
        if t >= 1. {
            known.going = None;
            return standing;
        }
        let mut frame = frame_at(
            going.was.clone(),
            going.strength,
            going.width.zip(known.width),
            t,
        );
        if going.since.is_none() {
            frame.width = going.width;
        }
        frame
    }

    /// A child that notes the width its parent was laid out at as the natural width of
    /// what `place` shows now.
    fn measure(&self, place: &SharedString, key: String) -> impl IntoElement + use<T> {
        let (places, place) = (self.0.clone(), place.clone());
        canvas(
            move |bounds, _, _| {
                if let Some(known) = places.borrow_mut().get_mut(&place)
                    && known.now.key() == key
                {
                    known.width = Some(bounds.size.width.as_f32());
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Words in `place` that swap: `draw` makes the element of what is shown.
    pub fn words(
        &self,
        place: impl Into<SharedString>,
        now: T,
        draw: impl Fn(&T) -> AnyElement,
        cx: &App,
    ) -> Div {
        let place = place.into();
        let key = now.key();
        let frame = self.frame(&place, now.clone(), cx.reduce_motion());
        // The new words stand in the flow at their own width, which is measured; the
        // room around them is what eases, and clips them at its sides meanwhile.
        let new = div()
            .relative()
            .flex_none()
            .whitespace_nowrap()
            .top(px(frame.below))
            .opacity(frame.new)
            .child(draw(&now))
            .child(self.measure(&place, key));
        let mut room = div().relative().flex().flex_row().items_center().min_w_0();
        if let Some(width) = frame.width {
            room = room.w(px(width)).overflow_x_hidden();
        }
        room = room.child(new);
        if let Some((was, strength)) = &frame.old {
            room = room.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(-frame.risen))
                    .bottom(px(frame.risen))
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .opacity(*strength)
                    .child(draw(was)),
            );
        }
        if frame.running {
            room = room.child(super::next_frame());
        }
        room
    }

    /// A mark in `place` that swaps within `slot`, which keeps its size: the old one
    /// fades out as the new one fades in over it.
    pub fn mark(
        &self,
        place: impl Into<SharedString>,
        now: T,
        slot: Div,
        draw: impl Fn(&T) -> AnyElement,
        cx: &App,
    ) -> Div {
        let frame = self.frame_of(&place.into(), now.clone(), cx.reduce_motion(), false);
        let layer = || {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
        };
        let mut slot = slot
            .relative()
            .child(layer().opacity(frame.new).child(draw(&now)));
        if let Some((was, strength)) = &frame.old {
            slot = slot.child(layer().opacity(*strength).child(draw(was)));
        }
        if frame.running {
            slot = slot.child(super::next_frame());
        }
        slot
    }
}

/// How far a swap that began at `since` is at `at`, from 0 to 1; 0 before it began.
fn progress(since: Option<Instant>, at: Instant) -> f32 {
    // A shot takes longer than a swap, so the screenshot tool names the moment itself.
    #[cfg(feature = "screenshot")]
    if let (Some(_), Ok(Ok(ms))) = (
        since,
        std::env::var("DUSK_SWAP").map(|ms| ms.parse::<f32>()),
    ) {
        return ms / 1000. / LENGTH.as_secs_f32();
    }
    since.map_or(0., |since| {
        at.saturating_duration_since(since).as_secs_f32() / LENGTH.as_secs_f32()
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui_kit::SharedString;

    use super::{LENGTH, Swaps, TRAVEL, frame_at};

    fn said(words: &'static str) -> SharedString {
        SharedString::from(words)
    }

    /// Lays `place` out at `width`, as a drawn frame does.
    fn laid_out(swaps: &Swaps<SharedString>, place: &SharedString, width: f32) {
        swaps.0.borrow_mut().get_mut(place).unwrap().width = Some(width);
    }

    #[test]
    fn the_first_words_and_the_same_words_again_stand_without_a_swap() {
        let (swaps, place) = (Swaps::default(), said("row"));
        for _ in 0..3 {
            let frame = swaps.frame(&place, said("Thinking"), false);
            assert!(frame.old.is_none() && !frame.running);
            assert_eq!((frame.new, frame.width), (1., None));
        }
    }

    #[test]
    fn new_words_wait_for_their_width_then_swap_in_the_room_between() {
        let (swaps, place) = (Swaps::default(), said("row"));
        swaps.frame(&place, said("Thinking"), false);
        laid_out(&swaps, &place, 50.);
        // The frame the change is seen on: the old words stand whole in the room they
        // had, the new are laid out unseen.
        let frame = swaps.frame(&place, said("Searching the project"), false);
        assert_eq!(frame.old, Some((said("Thinking"), 1.)));
        assert_eq!(
            (frame.new, frame.width, frame.running),
            (0., Some(50.), true)
        );
        laid_out(&swaps, &place, 130.);
        // From the next frame it runs, and ends with the new words standing alone.
        let frame = swaps.frame(&place, said("Searching the project"), false);
        assert!(
            frame.running
                && frame
                    .width
                    .is_some_and(|width| (50. ..=130.).contains(&width))
        );
        std::thread::sleep(LENGTH + Duration::from_millis(20));
        let frame = swaps.frame(&place, said("Searching the project"), false);
        assert!(frame.old.is_none() && !frame.running);
        assert_eq!((frame.new, frame.width), (1., None));
    }

    #[test]
    fn a_swap_goes_from_the_old_to_the_new_without_both_standing_strong() {
        let start = frame_at(said("old"), 1., Some((50., 130.)), 0.);
        assert_eq!(start.old, Some((said("old"), 1.)));
        assert_eq!((start.new, start.width), (0., Some(50.)));
        for step in 1..20 {
            let frame = frame_at(said("old"), 1., Some((50., 130.)), step as f32 / 20.);
            let old = frame.old.map_or(0., |(_, strength)| strength);
            assert!(
                old.min(frame.new) <= 0.25,
                "{old} and {} at once",
                frame.new
            );
            assert!(frame.risen <= TRAVEL && frame.below <= TRAVEL);
        }
        let end = frame_at(said("old"), 1., Some((50., 130.)), 1.);
        assert_eq!(end.old.map(|(_, strength)| strength), Some(0.));
        assert!((end.new - 1.).abs() < 1e-6 && end.below.abs() < 1e-6);
        assert_eq!(end.width, Some(130.));
    }

    #[test]
    fn words_that_come_mid_swap_take_over_and_nothing_queues() {
        let (swaps, place) = (Swaps::default(), said("row"));
        swaps.frame(&place, said("Thinking"), false);
        laid_out(&swaps, &place, 50.);
        swaps.frame(&place, said("Reading a file"), false);
        // Before the second words were even laid out, the third come: the second go
        // out from nothing, as they never showed, and the first are not seen again.
        let frame = swaps.frame(&place, said("Writing"), false);
        assert_eq!(frame.old, Some((said("Reading a file"), 0.)));
        assert_eq!(frame.width, Some(50.), "the room is still the first words'");
        laid_out(&swaps, &place, 40.);
        std::thread::sleep(LENGTH + Duration::from_millis(20));
        swaps.frame(&place, said("Writing"), false);
        std::thread::sleep(LENGTH + Duration::from_millis(20));
        let frame = swaps.frame(&place, said("Writing"), false);
        assert!(frame.old.is_none() && !frame.running, "one swap, then rest");
    }

    #[test]
    fn a_mark_swaps_at_once_as_its_slot_has_no_width_to_wait_for() {
        let (swaps, place) = (Swaps::default(), said("mark"));
        swaps.frame_of(&place, said("spark"), false, false);
        let frame = swaps.frame_of(&place, said("dot"), false, false);
        assert!(frame.running && frame.width.is_none());
        std::thread::sleep(LENGTH + Duration::from_millis(20));
        let frame = swaps.frame_of(&place, said("dot"), false, false);
        assert!(frame.old.is_none() && !frame.running);
    }

    #[test]
    fn under_reduced_motion_the_new_words_simply_stand() {
        let (swaps, place) = (Swaps::default(), said("row"));
        swaps.frame(&place, said("Thinking"), true);
        let frame = swaps.frame(&place, said("Writing"), true);
        assert!(frame.old.is_none() && !frame.running);
        assert_eq!((frame.new, frame.width), (1., None));
    }
}
