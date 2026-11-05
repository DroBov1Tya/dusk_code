//! The transcript drawn: messages as markdown, reasoning folded, tools as one line each.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::transcript::{Item, ToolState};
use crate::{widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (surface, muted, success, danger, accent, border) = (
        theme.muted,
        theme.muted_foreground,
        theme.success,
        theme.danger,
        theme.primary,
        theme.border,
    );
    let shown = ws.shown.clone().unwrap_or_default();
    let mut column = div().flex().flex_col().gap_3().px_6().py_4().max_w(px(900.)).mx_auto().w_full();
    let Some(chat) = ws.chats.get(&shown) else {
        return div().id("transcript").child(widgets::muted("Opening...", cx));
    };
    for (index, item) in chat.items.iter().enumerate() {
        let key = format!("{shown}-{index}");
        column = column.child(match item {
            Item::User(text) => div()
                .self_end()
                .max_w(px(640.))
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(surface)
                .child(text.clone())
                .into_any_element(),
            Item::Assistant { text, reasoning } => {
                let unfolded = ws.unfolded.contains(&(shown.clone(), index));
                let mut answer = div().flex().flex_col().gap_1();
                if !reasoning.trim().is_empty() {
                    let id = shown.clone();
                    answer = answer.child(
                        div()
                            .id(SharedString::from(format!("reasoning-{key}")))
                            .text_xs()
                            .text_color(muted)
                            .cursor_pointer()
                            .child(if unfolded { "▾ Thinking" } else { "▸ Thinking" })
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                let key = (id.clone(), index);
                                if !ws.unfolded.remove(&key) {
                                    ws.unfolded.insert(key);
                                }
                                cx.notify();
                            })),
                    );
                    if unfolded {
                        answer = answer.child(
                            div()
                                .pl_3()
                                .border_l_2()
                                .border_color(border)
                                .text_xs()
                                .text_color(muted)
                                .child(reasoning.clone()),
                        );
                    }
                }
                if !text.is_empty() {
                    answer = answer.child(super::links::markdown(
                        SharedString::from(format!("answer-{key}")),
                        text.clone(),
                    ));
                }
                answer.into_any_element()
            }
            Item::Tool(tool) => {
                let (icon, color) = match tool.state {
                    ToolState::Running => (IconName::LoaderCircle, accent),
                    ToolState::Done => (IconName::Check, success),
                    ToolState::Failed => (IconName::CircleX, danger),
                };
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(muted)
                    .child(widgets::icon(icon, color))
                    .child(div().font_weight(FontWeight::MEDIUM).child(tool.name.clone()))
                    .child(div().truncate().font_family("Menlo").child(tool.detail.clone()))
                    .into_any_element()
            }
            Item::Notice(text) => div().text_xs().text_color(muted).child(text.clone()).into_any_element(),
            Item::Error(text) => div()
                .px_3()
                .py_2()
                .rounded_md()
                .text_xs()
                .text_color(danger)
                .bg(danger.opacity(0.1))
                .child(text.clone())
                .into_any_element(),
        });
    }
    if chat.working {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .text_xs()
                .text_color(accent)
                .child(widgets::icon(IconName::LoaderCircle, accent))
                .child("Working..."),
        );
    }
    // Follows new output; while nothing changes the user scrolls freely.
    let size = (chat.items.len(), chat.working);
    if size != ws.followed {
        ws.followed = size;
        ws.scroll.scroll_to_bottom();
    } else if chat.working {
        ws.scroll.scroll_to_bottom();
    }
    div()
        .id("transcript")
        .size_full()
        .overflow_y_scroll()
        .track_scroll(&ws.scroll)
        .child(column)
}
