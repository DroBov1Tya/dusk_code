//! The Overview opened as a page: the column's sections in the same order, in two
//! columns; the event log takes the whole width under them.

use gpui_kit::{
    component::{
        IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::sections::{self, Form};
use crate::{ui, workspace::Workspace};

pub fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let mut sections = sections::all(ws, Form::Wide, cx);
    // The log is always last when it is there; it reads better across the page.
    let has_log = ws
        .overview
        .of(ws.shown.as_deref())
        .and_then(|shown| shown.overview.as_ref())
        .is_some_and(|overview| !overview.log.is_empty());
    let log = has_log.then(|| sections.pop()).flatten();
    let body = if ws.shown.is_none() {
        ui::empty(IconName::LayoutDashboard, "No chat is open", "", cx)
    } else if sections.is_empty() && log.is_none() {
        ui::loading("Reading the chat...", cx)
    } else {
        let (left, right): (Vec<_>, Vec<_>) = sections
            .into_iter()
            .enumerate()
            .partition(|(index, _)| index % 2 == 0);
        let column = |part: Vec<(usize, Div)>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_4()
                .children(part.into_iter().map(|(_, section)| section))
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap_4()
                    .child(column(left))
                    .child(column(right)),
            )
            .children(log)
    };
    div()
        .id("overview-page")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .max_w(px(980.))
                .mx_auto()
                .px_8()
                .py_6()
                .flex()
                .flex_col()
                .child(
                    div().pb_2().flex().flex_row().child(
                        Button::new("overview-back")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .label("Back")
                            .on_click(cx.listener(|ws, _, _, cx| ws.close_overview(cx))),
                    ),
                )
                .child(ui::page_title(
                    "Overview",
                    "How the shown chat's work stands",
                    cx,
                ))
                .child(body),
        )
}
