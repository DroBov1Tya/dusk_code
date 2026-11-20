//! What shows while the agent works, one look for each kind of work the protocol tells
//! apart (`dusk_protocol::thread::activity` chooses the kind; nothing here does):
//! thinking is a spark that eases from shape to shape beside a word with a light passing
//! through it; writing stands still, since the text arriving is the signal; planning
//! writes three lines; a running step has a dot with a ring going out from it; agents
//! are a dot each; waiting for the user is a steady ring; compacting draws three lines
//! together; a turn the provider holds is a ring that does not move.
//!
//! When the kind or its words change, the new take the old one's place in a short swap
//! (`swap`): never a hard cut, and what stands after the words glides to its new place.
//!
//! Every mark stands in one slot of the same size, so the row never moves when the kind
//! changes. What moves is drawn through `ui::looping`: one shared clock, at most thirty
//! frames a second, still under reduced motion, and gone with the row when nothing runs.

mod glyph;
mod shimmer;
mod swap;

pub use swap::Swaps;

use std::time::{Duration, Instant};

use dusk_protocol::thread::{ToolState, Work, WorkKind};
use gpui_kit::{
    component::{ActiveTheme, Colorize},
    *,
};

use crate::ui::looping;

/// The slot every kind's mark stands in.
const SLOT: f32 = 20.;

/// The colours the marks are drawn in, from the theme.
#[derive(Clone, Copy)]
struct Tones {
    accent: Hsla,
    text: Hsla,
    muted: Hsla,
    warning: Hsla,
    danger: Hsla,
}

fn tones(cx: &App) -> Tones {
    let theme = cx.theme();
    Tones {
        accent: theme.primary,
        text: theme.foreground,
        muted: theme.muted_foreground,
        warning: theme.warning,
        danger: theme.danger,
    }
}

/// What the row shows of the work, as the swaps tell one from the next.
#[derive(Clone)]
pub struct Said(pub Work);

impl swap::Shown for Said {
    /// The kind and its words. A held turn's words count down and agents come and go:
    /// neither is a change of what the row says.
    fn key(&self) -> String {
        match self.0.kind {
            WorkKind::Held => "held".to_owned(),
            WorkKind::Agents { .. } => "agents".to_owned(),
            kind => format!("{kind:?} {}", self.0.label),
        }
    }
}

/// The same work as a mark tells it: the kind alone, and whether it stalled.
#[derive(Clone)]
pub struct Marked(pub Work);

impl swap::Shown for Marked {
    fn key(&self) -> String {
        match self.0.kind {
            WorkKind::Agents { .. } => "agents".to_owned(),
            kind => format!("{kind:?} {}", self.0.stalled),
        }
    }
}

/// The places of a chat's working row that swap.
#[derive(Default)]
pub struct RowSwaps {
    pub words: Swaps<Said>,
    pub marks: Swaps<Marked>,
    /// The phrases of the running turn's headings, by the item each starts at.
    pub headings: Swaps<SharedString>,
}

impl RowSwaps {
    /// A new turn begins with nothing to swap from.
    pub fn clear(&self) {
        self.words.clear();
        self.marks.clear();
        self.headings.clear();
    }
}

fn slot() -> Div {
    div()
        .flex_none()
        .w(px(SLOT))
        .h(px(glyph::SIZE))
        .flex()
        .items_center()
        .justify_center()
}

/// The mark of `work`, in its slot.
pub fn mark(work: &Work, cx: &App) -> Div {
    slot().child(mark_of(work, cx))
}

/// The mark of `work` in its slot, taking the last mark's place in a swap.
pub fn mark_swapping(work: &Work, swaps: &RowSwaps, cx: &App) -> Div {
    swaps.marks.mark(
        "mark",
        Marked(work.clone()),
        slot(),
        |marked| mark_of(&marked.0, cx),
        cx,
    )
}

/// The words of `work`, taking the last words' place in a swap.
pub fn words_swapping(work: &Work, swaps: &RowSwaps, cx: &App) -> Div {
    swaps
        .words
        .words("words", Said(work.clone()), |said| words(&said.0, cx), cx)
}

/// A turn heading's phrase, taking the last one's place in a swap.
pub fn phrase_swapping(
    place: impl Into<SharedString>,
    phrase: SharedString,
    swaps: &RowSwaps,
    cx: &App,
) -> Div {
    swaps.headings.words(
        place,
        phrase,
        |phrase| phrase.clone().into_any_element(),
        cx,
    )
}

fn mark_of(work: &Work, cx: &App) -> AnyElement {
    let tones = tones(cx);
    match work.kind {
        WorkKind::Thinking => {
            // A stalled turn turns toward the warning colour; the spark goes on.
            let color = match work.stalled {
                true => tones.accent.mix_oklab(tones.warning, 0.75),
                false => tones.accent,
            };
            glyph::spark(color).into_any_element()
        }
        WorkKind::Writing => glyph::still_dot(tones.muted.opacity(0.7)).into_any_element(),
        WorkKind::Planning => glyph::lines(tones.accent, tones.muted).into_any_element(),
        WorkKind::Steps => glyph::live(tones.accent).into_any_element(),
        WorkKind::Agents { working, started } => {
            glyph::dots(working, started, tones.accent, tones.muted).into_any_element()
        }
        WorkKind::WaitingForUser => glyph::ring(tones.warning, false).into_any_element(),
        WorkKind::Compacting => glyph::squeeze(tones.muted, tones.text).into_any_element(),
        WorkKind::Held => glyph::ring(tones.warning.opacity(0.7), true).into_any_element(),
    }
}

/// The words of `work`: a light passes through them while the turn thinks; a turn that
/// waits for the user says so in the text's own colour.
pub fn words(work: &Work, cx: &App) -> AnyElement {
    let tones = tones(cx);
    let label = SharedString::from(work.label.clone());
    match work.kind {
        WorkKind::Thinking if !work.stalled => {
            shimmer::word(label, tones.muted, tones.text).into_any_element()
        }
        WorkKind::WaitingForUser => div().text_color(tones.text).child(label).into_any_element(),
        _ => label.into_any_element(),
    }
}

/// The plan strip's label: it carries the light while the plan is written.
pub fn plan_label(planning: bool, cx: &App) -> AnyElement {
    let tones = tones(cx);
    match planning {
        true => shimmer::word("Plan".into(), tones.muted, tones.text).into_any_element(),
        false => SharedString::from("Plan").into_any_element(),
    }
}

/// How long a finished step's check or cross takes to settle in.
const SETTLE: Duration = Duration::from_millis(180);

/// The state of a step at the end of its row: the live mark while it runs, a check once
/// it is done, a cross when it failed. One that finished at `settled` grows and fades in
/// once and then stands still, as does a step its turn left running: `turn_runs` says
/// whether anything can still end it.
pub fn step_mark(state: ToolState, turn_runs: bool, settled: Option<Instant>, cx: &App) -> Div {
    let tones = tones(cx);
    let slot = div().flex_none().size(px(glyph::SIZE));
    let settling = settled.and_then(|at| arriving(at, SETTLE, cx));
    let t = settling.unwrap_or(1.);
    let slot = match state {
        ToolState::Running if !turn_runs => return slot,
        ToolState::Running => return slot.child(glyph::live(tones.accent)),
        ToolState::Done => slot.child(glyph::check(tones.muted.opacity(t), 0.6 + 0.4 * t)),
        ToolState::Failed => slot.child(glyph::cross(tones.danger.opacity(t), 0.6 + 0.4 * t)),
    };
    match settling {
        Some(_) => slot.child(next_frame()),
        None => slot,
    }
}

/// The live mark alone, for the heading of a turn whose step runs.
pub fn live_mark(cx: &App) -> Div {
    div()
        .flex_none()
        .size(px(glyph::SIZE))
        .child(glyph::live(tones(cx).accent))
}

/// Draws the view again once a second, so a clock ticks beside a mark that stands still.
/// It adds no frame of its own while a mark moves: its tick is one of theirs.
pub fn second_tick() -> impl IntoElement {
    canvas(
        |_, window, cx| looping::ask_redraw(looping::FPS, window, cx),
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// Draws the view again on the next tick of the shared clock: the child of something
/// that plays once (`arriving`), for as long as it plays.
pub fn next_frame() -> impl IntoElement {
    canvas(
        |_, window, cx| looping::ask_redraw(1, window, cx),
        |_, _, _, _| {},
    )
    .absolute()
    .size_0()
}

/// How far something that came at `since` is into arriving, eased, or none once it is
/// there or when the system asks for less motion. Whoever draws by it adds `next_frame`.
pub fn arriving(since: Instant, length: Duration, cx: &App) -> Option<f32> {
    let passed = Instant::now().saturating_duration_since(since);
    if cx.reduce_motion() || passed >= length {
        return None;
    }
    Some(crate::ui::motion::ease_out(
        passed.as_secs_f32() / length.as_secs_f32(),
    ))
}
