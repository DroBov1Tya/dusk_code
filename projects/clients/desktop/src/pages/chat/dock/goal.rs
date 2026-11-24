//! The chat's standing goal over the composer: a quiet "◎ goal active · 12m" that opens a
//! small card of the condition, what it took so far, the evaluator's newest reason and the
//! way to clear it. Clearing sends `/goal clear`, so the core does it as it does when typed.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        tooltip::Tooltip,
    },
    *,
};

use dusk_protocol::{
    system::now_ms,
    thread::{GoalStatus, plural, short_count},
};

use crate::{
    ui::{
        Type,
        elevation::{Elevation, Level},
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// The condition's lines before it is cut; the whole of it is under the pointer.
const CONDITION_LINES: usize = 3;
/// How wide the condition's whole text stands under the pointer.
const TIP_WIDTH: f32 = 420.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let goal = ws.shown_chat()?.goal.as_ref()?;
    let now = now_ms();
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    // A paused goal waits for the user: its words take the accent, an active one stays calm.
    let tint = match goal.status {
        GoalStatus::Active => muted,
        GoalStatus::Paused => theme.warning,
    };
    let open = ws.dock.goal_open;
    let indicator = div()
        .id("goal-indicator")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .focusable_row(cx)
        .aria_expanded(open)
        .small()
        .text_color(tint)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(goal.indicator(now)),
        )
        .child(widgets::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            muted,
        ))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_goal(cx)));
    if !open {
        let hover = theme.list_hover;
        return Some(
            crate::pages::chat::composer::dock_lane(
                div().flex().child(
                    indicator
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .hover(move |pill| pill.bg(hover))
                        .tooltip(|window, cx| Tooltip::new("Show the goal").build(window, cx)),
                ),
            )
            .pb_2(),
        );
    }
    let condition = goal.text.clone();
    // The time is in the head already.
    let spent = format!(
        "{} evaluated \u{00B7} {} tokens",
        plural(u64::from(goal.turns), "turn"),
        short_count(goal.tokens)
    );
    let line = |text: String| div().small().text_color(muted).child(text);
    let card = div()
        .flex()
        .flex_col()
        .gap_2()
        .px_5()
        .pt_3()
        .pb_3()
        .rounded_xl()
        .elevation(Level::Raised, cx)
        .child(indicator)
        .child(
            div()
                .id("goal-condition")
                .body()
                .line_clamp(CONDITION_LINES)
                .child(condition.clone())
                .tooltip(move |window, cx| {
                    Tooltip::new(condition.clone())
                        .max_w(px(TIP_WIDTH))
                        .build(window, cx)
                }),
        )
        .child(line(spent))
        .children(goal.note.as_ref().map(|note| {
            div()
                .small()
                .text_color(tint)
                .child(format!("Paused: {note}"))
        }))
        .children(goal.reason.as_ref().map(|reason| {
            div()
                .pt_2()
                .border_t_1()
                .border_color(border.opacity(0.5))
                .small()
                .text_color(muted)
                .line_clamp(CONDITION_LINES)
                .child(format!("Latest: {reason}"))
        }))
        .child(
            div().flex().justify_end().child(
                Button::new("goal-clear")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .label("Clear goal")
                    .tooltip("Stop working toward it, as /goal clear does")
                    .on_click(cx.listener(|ws, _, _, cx| ws.clear_goal(cx))),
            ),
        );
    Some(crate::pages::chat::composer::dock_lane(card).pb_2())
}

impl Workspace {
    pub fn toggle_goal(&mut self, cx: &mut Context<Self>) {
        self.dock.goal_open = !self.dock.goal_open;
        cx.notify();
    }

    /// Clears the shown chat's goal the way `/goal clear` does; the core's answer takes the
    /// goal away. A turn under way ends first, as for a typed command.
    pub fn clear_goal(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.dock.goal_open = false;
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .send(&thread_id, "/goal clear".into(), false)
                    .await
            },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}
