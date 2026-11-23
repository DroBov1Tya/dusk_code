//! The pull requests of a chat beside its title: an icon and their count, tinted by the
//! worst checks of the open ones, and under the pointer a card that lists them, the last
//! linked first, merged and closed ones too. A row of the card opens the pull request's
//! page. The core keeps the list (`Thread::pulls`); nothing is read here.

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

/// The icon and count when the chat has pull requests; nothing when it has none.
pub fn badge(thread: &Thread, prefix: &str, cx: &App) -> Option<AnyElement> {
    if thread.pulls.is_empty() {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let color = worst_checks(&thread.pulls).map_or(muted, |state| check_color(state, cx));
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
                .child(thread.pulls.len().to_string()),
        );
    let pulls: Vec<ThreadPull> = thread.pulls.iter().rev().cloned().collect();
    Some(
        HoverCard::new(SharedString::from(format!("{prefix}-pulls-{}", thread.id)))
            .anchor(Anchor::TopLeft)
            .open_delay(Duration::from_millis(300))
            .trigger(trigger)
            .content(move |_, _, cx| card(&pulls, cx))
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

fn card(pulls: &[ThreadPull], cx: &App) -> Div {
    let theme = cx.theme();
    let mut list = div()
        .id("chat-pulls-list")
        .max_h(px(CARD_HEIGHT))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_0p5();
    for (index, kept) in pulls.iter().enumerate() {
        list = list.child(row(index, kept, cx));
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
        .child(list)
}

/// The title over the state, the repository and number, and the size; a pull request
/// the core has not read yet shows its number only and opens nothing.
fn row(index: usize, kept: &ThreadPull, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let place = div()
        .min_w_0()
        .truncate()
        .text_color(muted)
        .child(format!("{} #{}", kept.repository, kept.number));
    let mut facts = div().flex().flex_row().items_center().gap_1p5().meta();
    let title = match &kept.pull {
        Some(pull) => {
            let color = state_color(pull.state, cx);
            facts = facts
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
                .child(place)
                .child(div().flex_1())
                .child(size(pull, cx));
            div().child(pull.title.clone())
        }
        None => {
            facts = facts.child(place);
            div()
                .text_color(muted)
                .child(format!("Pull request #{}", kept.number))
        }
    };
    let url = kept
        .pull
        .as_ref()
        .map(|pull| pull.url.clone())
        .filter(|url| url.starts_with("https://"));
    widgets::row(("chat-pull", index), false, cx)
        .mx_0()
        .h_auto()
        .py_1p5()
        .flex_col()
        .items_stretch()
        .gap_0p5()
        .child(title.small().truncate())
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
