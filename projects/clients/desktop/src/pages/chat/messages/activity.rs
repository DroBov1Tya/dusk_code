//! A turn's activity folded into one row, and the foldable texts inside it: thoughts,
//! agent reports, background results.

use gpui_kit::{
    component::{ActiveTheme, Colorize, IconName},
    prelude::FluentBuilder,
    *,
};

use super::{
    ACTIVITY,
    picture::picture,
    step::{background_button, step},
};
use crate::{
    pages::chat::{
        links,
        transcript::{Item, ToolState},
    },
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

pub fn turn_activity(
    ws: &Workspace,
    shown: &str,
    start: usize,
    items: &[Item],
    span: Option<crate::pages::chat::timing::Span>,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), start | ACTIVITY));
    let mut latest = None;
    let mut running = None;
    for (index, item) in items.iter().enumerate().rev() {
        if let Item::Tool(tool) = item {
            if latest.is_none() {
                latest = Some(tool);
            }
            if running.is_none() && tool.state == ToolState::Running && tool.name == "run_command" {
                running = Some((start + index, tool));
            }
        }
        if latest.is_some() && running.is_some() {
            break;
        }
    }
    // A finished turn says how long it took; one under way, what it does now.
    let title = match span {
        Some((began, Some(ended))) => {
            format!(
                "Worked for {}",
                crate::pages::chat::timing::duration(ended - began)
            )
        }
        _ => latest
            .map_or("Thinking", |tool| {
                dusk_protocol::tools::phrase(&tool.name, &tool.call_id)
            })
            .to_owned(),
    };
    let id = shown.to_owned();
    let foot_id = id.clone();
    let foot_title = title.clone();
    let header = disclosure(
        SharedString::from(format!("activity-{shown}-{start}")),
        open,
        cx,
    )
    .flex_1()
    .aria_label(format!("Turn activity, {title}"))
    .child(disclosure_icon(open, step_muted(cx)))
    .child(
        div()
            .min_w_0()
            .truncate()
            .text_color(cx.theme().foreground)
            .child(title),
    )
    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | ACTIVITY, cx)));
    let mut row = div().flex().items_center().gap_2().child(header);
    if !open && let Some((index, tool)) = running {
        row = row.child(background_button(shown, index, tool, cx));
    }
    let mut block = div().flex().flex_col().gap_1().child(row);
    if open {
        let mut detail = div()
            .ml_4()
            .pl_2()
            .border_l_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .gap_1();
        for (offset, item) in items.iter().enumerate() {
            let index = start + offset;
            detail = match item {
                Item::Tool(tool) => detail.child(step(ws, shown, index, tool, cx)),
                Item::Assistant { text, reasoning } => {
                    let mut detail = detail;
                    if !reasoning.trim().is_empty() {
                        detail = detail.child(activity_text(
                            ws,
                            shown,
                            index,
                            ("Thinking", IconName::Asterisk),
                            reasoning,
                            || reasoning.clone().into_any_element(),
                            cx,
                        ));
                    }
                    // A running turn shows what it says in the chat, not here.
                    let running = matches!(span, Some((_, None)) | None);
                    if !text.trim().is_empty() && !running {
                        detail = detail.child(div().px_2().small().child(
                            crate::pages::chat::links::markdown(
                                SharedString::from(format!("said-{shown}-{index}")),
                                text.clone(),
                            ),
                        ));
                    }
                    detail
                }
                Item::Agent { from, text } => {
                    detail.child(agent_report(ws, shown, index, from, text, cx))
                }
                Item::Image(image) => detail.child(picture(&ws.status.artifacts_dir, image, cx)),
                Item::Notice(text) => detail.child(
                    div()
                        .px_2()
                        .small()
                        .text_color(step_muted(cx))
                        .child(text.clone()),
                ),
                Item::User {
                    text,
                    sequence: None,
                } => detail.child(activity_text(
                    ws,
                    shown,
                    index,
                    ("Background task result", IconName::SquareTerminal),
                    text,
                    || text.clone().into_any_element(),
                    cx,
                )),
                _ => detail,
            };
        }
        block = block.child(widgets::unfold(
            SharedString::from(format!("unfold-{shown}-{start}")),
            detail,
        ));
        if items.len() > 3 {
            block = block.child(
                fold_foot(
                    SharedString::from(format!("activity-foot-{shown}-{start}")),
                    foot_title,
                    cx,
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&foot_id, start | ACTIVITY, cx))),
            );
        }
    }
    block
}

pub fn agent_report(
    ws: &Workspace,
    shown: &str,
    index: usize,
    from: &str,
    text: &str,
    cx: &mut Context<Workspace>,
) -> Div {
    let label = if ws.unfolded.contains(&(shown.to_owned(), index)) {
        format!("{from} reported")
    } else {
        let first = text.lines().next().unwrap_or_default();
        format!("{from} reported: {first}")
    };
    activity_text(
        ws,
        shown,
        index,
        (&label, IconName::Bot),
        text,
        || {
            links::markdown(
                SharedString::from(format!("report-{shown}-{index}")),
                text.to_owned(),
            )
            .into_any_element()
        },
        cx,
    )
}

pub fn activity_text(
    ws: &Workspace,
    shown: &str,
    index: usize,
    (title, icon): (&str, IconName),
    text: &str,
    body: impl FnOnce() -> AnyElement,
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), index));
    let long = long(text);
    let id = shown.to_owned();
    let foot_id = id.clone();
    let muted = step_muted(cx);
    let group = SharedString::from(format!("activity-text-row-{shown}-{index}"));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .group(group.clone())
                .flex()
                .items_center()
                .gap_1()
                .child(
                    disclosure(
                        SharedString::from(format!("activity-text-{shown}-{index}")),
                        open,
                        cx,
                    )
                    .flex_1()
                    .aria_label(title.to_owned())
                    .child(widgets::icon(icon, muted))
                    .child(div().flex_1().min_w_0().truncate().child(title.to_owned()))
                    .child(disclosure_icon(open, muted))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index, cx))),
                )
                .child(widgets::copy(
                    SharedString::from(format!("activity-text-copy-{shown}-{index}")),
                    &group,
                    text,
                )),
        )
        .when(open, |block| {
            block.child(
                div()
                    .ml_4()
                    .pl_3()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .small()
                    .text_color(muted)
                    .child(body()),
            )
        })
        .when(open && long, |block| {
            block.child(
                fold_foot(
                    SharedString::from(format!("activity-text-foot-{shown}-{index}")),
                    title.to_owned(),
                    cx,
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&foot_id, index, cx))),
            )
        })
}

pub fn step_muted(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.foreground.mix_oklab(theme.muted_foreground, 0.16)
}

pub fn disclosure(id: impl Into<ElementId>, open: bool, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().list_hover;
    div()
        .id(id)
        .focusable_row(cx)
        .role(Role::Button)
        .aria_expanded(open)
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(28.))
        .min_w_0()
        .px_2()
        .rounded_md()
        .small()
        .text_color(step_muted(cx))
        .cursor_pointer()
        .when(open, |row| row.bg(hover))
        .hover(move |row| row.bg(hover))
}

/// Whether an unfolded text is long enough that its end is far from its heading.
pub fn long(text: &str) -> bool {
    text.len() > 500 || text.lines().count() > 8
}

/// The heading again under a long unfolded block, so it folds from where the reading
/// ended instead of after a scroll back up to where it began.
pub fn fold_foot(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    disclosure(id, false, cx)
        .aria_label("Fold")
        .child(widgets::icon(IconName::ChevronUp, step_muted(cx)))
        .child(div().min_w_0().truncate().child(title.into()))
}

pub fn disclosure_icon(open: bool, color: Hsla) -> impl IntoElement {
    widgets::icon(
        if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        },
        color,
    )
}

impl Workspace {
    pub fn fold(&mut self, chat: &str, key: usize, cx: &mut Context<Self>) {
        let key = (chat.to_owned(), key);
        if !self.unfolded.remove(&key) {
            self.unfolded.insert(key);
        }
        self.measure_again = true;
        cx.notify();
    }
}
