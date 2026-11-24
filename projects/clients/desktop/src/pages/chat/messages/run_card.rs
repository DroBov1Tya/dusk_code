//! A run the chat started, as a card where it began: its title and a chevron, "Team ·
//! 5 agents · 1m 12s", and a square a task or agent, as the plan card draws them. A
//! click opens the run's details: the right column's Team screen for a team, the agents
//! window for subagents. The protocol says what the card shows (`ChatLog::run_card`).

use dusk_protocol::{
    system::now_ms,
    thread::{RunCard, RunKind, RunMark},
};
use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{
    pages::{
        chat::{activity, transcript::Transcript, working::tabular},
        team::waves,
    },
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::{Right, Workspace},
};

/// The widest a card stands, so a short run does not stretch across the lane.
const MAX_W: f32 = 420.;

pub fn render(
    chat: &Transcript,
    shown: &str,
    key: &str,
    at: usize,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let Some(card) = chat.run_card(at, now_ms()) else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let surface = theme.foreground.opacity(0.035);
    let (kind, thread) = (card.kind, shown.to_owned());
    div()
        .id(SharedString::from(format!("run-{key}")))
        .w_full()
        .max_w(px(MAX_W))
        .flex()
        .flex_col()
        .gap_1()
        .px_4()
        .py_3()
        .rounded_xl()
        .bg(surface)
        .cursor_pointer()
        .focusable_row(cx)
        .hover(move |card| card.bg(hover))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .child(card.title.clone()),
                )
                .child(widgets::icon(IconName::ChevronRight, muted)),
        )
        .child(
            div()
                .small()
                .text_color(muted)
                .font_features(tabular())
                .child(card.line()),
        )
        .children((!card.marks.is_empty()).then(|| marks(&card, cx)))
        // Only a running card's clock moves: it draws again once a second, its height
        // the same, so the list measures nothing anew.
        .children(card.running.then(activity::second_tick))
        .on_click(cx.listener(move |ws, _, window, cx| match kind {
            RunKind::Team => {
                ws.right_hidden = false;
                ws.open_right(Right::Team, cx);
            }
            RunKind::Subagents => {
                ws.modal = Some(crate::modal::Modal::Agents {
                    thread_id: thread.clone(),
                });
                ws.modal_focus.focus(window, cx);
                cx.notify();
            }
        }))
        .into_any_element()
}

/// A square a task or agent; what runs breathes, as on the plan card.
fn marks(card: &RunCard, cx: &App) -> Div {
    let mut row = div().flex().flex_row().flex_wrap().gap_1().pt_1();
    for &mark in &card.marks {
        let square = waves::mark_square(mark, cx);
        row = match mark {
            RunMark::Running => row.child(waves::breathe(square)),
            _ => row.child(square),
        };
    }
    row
}
