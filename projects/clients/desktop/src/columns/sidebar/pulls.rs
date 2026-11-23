//! The pull requests of a chat beside its title: an icon and their count, tinted by the
//! worst checks of the open ones, and under the pointer a card that lists them, the last
//! linked first, merged and closed ones too. Only the ones the core has read are shown:
//! one not read yet has nothing to say. A row of the card opens the pull request's page.
//! The core keeps the list (`Thread::pulls`); nothing is read here.

use std::time::Duration;

use dusk_protocol::{
    project::{CheckState, PullState, PullStatus, ThreadPull},
    thread::Thread,
};
use gpui_kit::{
    component::{ActiveTheme, Icon, hover_card::HoverCard},
    prelude::FluentBuilder,
    *,
};

use crate::{
    columns::inspector::pulls::{check_color, grouped, state_icon},
    pages::chat::pull_link::state_word,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The card stops growing here and scrolls.
const CARD_HEIGHT: f32 = 360.;
const CARD_WIDTH: f32 = 340.;
/// The fade over the bottom of a card that scrolls on.
const FADE_HEIGHT: f32 = 28.;

/// The icon and count when the chat has pull requests the core has read; else nothing.
pub fn badge(thread: &Thread, prefix: &str, cx: &App) -> Option<AnyElement> {
    let pulls: Vec<ThreadPull> = thread
        .pulls
        .iter()
        .rev()
        .filter(|kept| kept.pull.is_some())
        .cloned()
        .collect();
    if pulls.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let color = worst_checks(&pulls).map_or(muted, |state| check_color(state, cx));
    let trigger = div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap_0p5()
        .child(
            Icon::default()
                .path(crate::ui::icons::GIT_PULL)
                .size_3()
                .text_color(color),
        )
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(pulls.len().to_string()),
        );
    let id = SharedString::from(format!("{prefix}-pulls-{}", thread.id));
    Some(
        HoverCard::new(id.clone())
            .anchor(Anchor::TopLeft)
            .open_delay(Duration::from_millis(300))
            .trigger(trigger)
            .content(move |_, window, cx| card(&id, &pulls, window, cx))
            .into_any_element(),
    )
}

/// Failing over pending over passing, of the pull requests still open; `None` when none
/// of them has checks.
fn worst_checks(pulls: &[ThreadPull]) -> Option<CheckState> {
    pulls
        .iter()
        .filter_map(|kept| kept.pull.as_ref())
        .filter(|pull| matches!(pull.state, PullState::Open | PullState::Draft))
        .filter_map(|pull| pull.checks.state)
        .max_by_key(|state| match state {
            CheckState::Passing => 0,
            CheckState::Pending => 1,
            CheckState::Failing => 2,
        })
}

/// The list, faded at the bottom while more of it is below: the scroll is kept across
/// frames, and the first frame asks for a second, once the list's height is known.
fn card(id: &SharedString, pulls: &[ThreadPull], window: &mut Window, cx: &mut App) -> Div {
    let scroll = window
        .use_keyed_state(id.clone(), cx, |_, _| ScrollHandle::new())
        .read(cx)
        .clone();
    if scroll.bounds().size.height <= px(0.) {
        window.request_animation_frame();
    }
    let more_below = scroll.offset().y > -scroll.max_offset().y;
    let theme = cx.theme();
    let mut list = div()
        .id("chat-pulls-list")
        .track_scroll(&scroll)
        .max_h(px(CARD_HEIGHT))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_0p5();
    let read = pulls
        .iter()
        .filter_map(|kept| Some((&kept.repository, kept.pull.as_ref()?)));
    for (index, (repository, pull)) in read.enumerate() {
        list = list.child(row(index, repository, pull, cx));
    }
    div()
        .w(px(CARD_WIDTH))
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .text_color(theme.foreground)
        .child(
            div()
                .px_2()
                .pt_0p5()
                .meta()
                .text_color(theme.muted_foreground)
                .child(widgets::count(pulls.len(), "pull request")),
        )
        .child(div().relative().child(list).when(more_below, |wrap| {
            wrap.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(FADE_HEIGHT))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(theme.popover, 0.),
                        linear_color_stop(theme.popover.opacity(0.), 1.),
                    )),
            )
        }))
}

/// The title over the state, the repository and number, and the size.
fn row(index: usize, repository: &str, pull: &PullStatus, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let color = state_color(pull.state, cx);
    let facts = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .meta()
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap_1()
                .text_color(color)
                .font_weight(FontWeight::MEDIUM)
                .child(state_icon(pull.state).size_3().text_color(color))
                .child(state_word(pull.state)),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(format!("{repository} #{}", pull.number)),
        )
        .child(div().flex_1())
        .child(size(pull, cx));
    let url = Some(pull.url.clone()).filter(|url| url.starts_with("https://"));
    widgets::row(("chat-pull", index), false, cx)
        .mx_0()
        .h_auto()
        .py_1p5()
        .flex_col()
        .items_stretch()
        .gap_0p5()
        .child(div().child(pull.title.clone()).small().truncate())
        .child(facts)
        .when_none(&url, |row| row.cursor_default())
        .when_some(url, |row, url| {
            row.on_click(move |_, _, cx: &mut App| cx.open_url(&url))
        })
}

/// "+12 −3", added green and removed red.
fn size(pull: &PullStatus, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .flex_none()
        .gap_1()
        .child(
            div()
                .text_color(theme.success)
                .child(format!("+{}", grouped(pull.added))),
        )
        .child(
            div()
                .text_color(theme.danger)
                .child(format!("\u{2212}{}", grouped(pull.removed))),
        )
}

/// Merged in violet and open in green; what is left of a draft or a closed one is quiet.
fn state_color(state: PullState, cx: &App) -> Hsla {
    let theme = cx.theme();
    match state {
        PullState::Open => theme.success,
        PullState::Merged => theme.magenta,
        PullState::Draft | PullState::Closed => theme.muted_foreground,
    }
}

impl Workspace {
    /// The core told chat `thread_id`'s pull requests anew.
    pub fn pulls_heard(&mut self, thread_id: &str, pulls: &[ThreadPull]) {
        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == thread_id)
        {
            thread.pulls = pulls.to_vec();
        }
    }
}
