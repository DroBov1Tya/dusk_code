//! While you were away: the scheduled tasks that ran since the user last looked, on one
//! card at the top of the chat, each with a way to its chat. "Got it" puts it away until
//! the next runs.

use chrono::Utc;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    ui::{Type, theme, widgets},
    workspace::{Center, Workspace},
};

/// The card, when tasks ran since the user last put it away.
pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let seen = theme::prefs(cx).runs_seen_until;
    let tasks = ws.schedules.as_ref()?;
    let ran: Vec<_> = tasks
        .iter()
        .filter(|task| task.last_run_at.is_some_and(|at| at > seen))
        .collect();
    if ran.is_empty() || seen == 0 {
        return None;
    }
    let theme = cx.theme();
    let (border, muted, accent) = (theme.border, theme.muted_foreground, theme.primary);
    let mut card = div()
        .mx_4()
        .mt_3()
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_xl()
        .border_1()
        .border_color(accent.opacity(0.3))
        .bg(accent.opacity(0.06))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(widgets::icon(IconName::Moon, accent))
                .child(
                    div()
                        .flex_1()
                        .body()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!(
                            "While you were away: {}",
                            widgets::count(ran.len(), "task")
                        )),
                )
                .child(
                    Button::new("report-seen")
                        .ghost()
                        .small()
                        .label("Got it")
                        .on_click(cx.listener(|_, _, _, cx| {
                            let now = Utc::now().timestamp_millis();
                            theme::change(cx, |prefs| prefs.runs_seen_until = now);
                        })),
                ),
        );
    for (index, task) in ran.into_iter().enumerate() {
        let title = task.prompt.lines().next().unwrap_or_default().to_owned();
        let project = ws
            .projects
            .iter()
            .find(|project| project.id == task.project_id)
            .map_or("a removed project", |project| project.name.as_str());
        let when = task.last_run_at.map(super::moment).unwrap_or_default();
        let thread = task.last_thread_id.clone();
        card = card.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .pt_1()
                .border_t_1()
                .border_color(border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(title))
                        .child(
                            div()
                                .meta()
                                .text_color(muted)
                                .child(format!("{project} \u{00B7} ran {when}")),
                        ),
                )
                .children(thread.map(|thread_id| {
                    Button::new(("report-open", index))
                        .outline()
                        .small()
                        .label("Open")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.center = Center::Chat;
                            ws.open_chat(&thread_id, cx);
                        }))
                })),
        );
    }
    Some(card)
}
