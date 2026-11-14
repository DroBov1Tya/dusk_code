//! A rail place shown while the pointer rests on its icon: the chats when the sidebar is
//! hidden, the timetable from any other page. It floats beside the rail, closes a moment
//! after the pointer leaves both, and a click on the icon opens the place for good.

use std::time::Duration;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// How long the panel waits for the pointer to come back before it closes.
const LINGER: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flyout {
    Chats,
    Schedules,
}

/// The panel, when one is up; drawn over the work, beside the rail.
pub fn render(
    ws: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Option<Deferred> {
    let kind = ws.flyout?;
    let body = match kind {
        Flyout::Chats => crate::columns::sidebar::render(ws, window, cx),
        Flyout::Schedules => schedules(ws, cx),
    };
    let theme = cx.theme();
    let panel = div()
        .id("flyout")
        .absolute()
        .top(px(crate::columns::topbar::HEIGHT + 4.))
        .bottom(px(8.))
        .left(px(crate::columns::rail::width(ws) + 6.))
        .w(px(crate::columns::sidebar::WIDTH))
        .rounded_xl()
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .shadow_lg()
        .occlude()
        .on_hover(cx.listener(|ws, inside: &bool, _, cx| ws.hover_flyout(None, *inside, cx)))
        .child(body);
    Some(widgets::on_top(widgets::appear("flyout-fade", panel)))
}

/// The timetable at a glance: a way to add a task and the next few.
fn schedules(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut column = div()
        .w(px(crate::columns::sidebar::WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .child(
            div()
                .h(px(40.))
                .px_2()
                .flex()
                .items_center()
                .title()
                .child("Scheduled"),
        )
        .child(
            widgets::row("flyout-new-task", false, cx)
                .child(widgets::icon(IconName::Plus, muted))
                .child("New task")
                .on_click(cx.listener(|ws, _, window, cx| {
                    ws.flyout = None;
                    ws.open_schedules(cx);
                    ws.open_schedule_editor(None, window, cx);
                })),
        );
    match &ws.schedules {
        Some(tasks) if !tasks.is_empty() => {
            for (index, task) in tasks.iter().take(12).enumerate() {
                let title = task.prompt.lines().next().unwrap_or_default().to_owned();
                let when = task.repeat.describe();
                column = column.child(
                    widgets::row(("flyout-task", index), false, cx)
                        .h(px(48.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().child(title))
                                .child(div().meta().text_color(muted).truncate().child(when)),
                        )
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.flyout = None;
                            ws.open_schedules(cx);
                        })),
                );
            }
        }
        Some(_) => {
            column = column.child(
                div()
                    .px_2()
                    .small()
                    .text_color(muted)
                    .child("No scheduled tasks"),
            );
        }
        None => column = column.child(widgets::muted("Reading...", cx)),
    }
    column
}

impl Workspace {
    /// The pointer came onto a rail icon or the panel (`inside`), or left one. Leaving
    /// waits a moment, so the pointer can cross from the icon to the panel.
    pub fn hover_flyout(&mut self, kind: Option<Flyout>, inside: bool, cx: &mut Context<Self>) {
        self.flyout_inside = inside;
        if inside {
            if let Some(kind) = kind {
                if kind == Flyout::Schedules && self.schedules.is_none() {
                    self.load_schedules(cx);
                }
                self.flyout = Some(kind);
            }
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LINGER).await;
            let _ = this.update(cx, |ws, cx| {
                if !ws.flyout_inside {
                    ws.flyout = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
