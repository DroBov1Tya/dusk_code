//! Screenshots the model took of the screen, newest first, as a list: a small thumbnail
//! and its name on each row, the first few with "View all" for the rest. A click opens
//! one large over the window.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{
    modal::Modal,
    pages::chat::messages::picture_path,
    ui::{Type, widgets},
    workspace::{Right, Workspace},
};

/// Rows shown before "View all".
const FIRST: usize = 5;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let shots = ws
        .shown_chat()
        .map(|chat| chat.screenshots.as_slice())
        .unwrap_or_default();
    if shots.is_empty() {
        return div().child(widgets::muted("Screenshots the model takes show here.", cx));
    }
    let theme = cx.theme();
    let (surface, muted) = (theme.muted, theme.muted_foreground);
    let all = ws.unfolded_modules.contains(&Right::Screenshots);
    let total = shots.len();
    let mut list = div().flex().flex_col();
    for (index, shot) in shots
        .iter()
        .rev()
        .enumerate()
        .take(if all { total } else { FIRST })
    {
        let Some(path) = picture_path(&ws.status.artifacts_dir, &shot.id) else {
            continue;
        };
        let name = shot
            .title
            .clone()
            .unwrap_or_else(|| format!("Screenshot {}", total - index));
        let large = path.clone();
        list = list.child(
            widgets::row(("screenshot", index), false, cx)
                .h(px(36.))
                .child(
                    div()
                        .size(px(24.))
                        .flex_none()
                        .rounded_sm()
                        .overflow_hidden()
                        .bg(surface)
                        .child(img(path).size_full().object_fit(ObjectFit::Cover)),
                )
                .child(div().flex_1().min_w_0().truncate().small().child(name))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(Modal::Picture {
                        path: large.clone(),
                    });
                    cx.notify();
                })),
        );
    }
    if total > FIRST {
        list = list.child(
            widgets::row("screenshots-all", false, cx)
                .h(px(32.))
                .text_color(muted)
                .child(widgets::icon(
                    if all {
                        IconName::ChevronUp
                    } else {
                        IconName::Ellipsis
                    },
                    muted,
                ))
                .child(div().small().child(if all {
                    "Show fewer".to_owned()
                } else {
                    format!("View all {total}")
                }))
                .on_click(cx.listener(|ws, _, _, cx| {
                    if !ws.unfolded_modules.remove(&Right::Screenshots) {
                        ws.unfolded_modules.insert(Right::Screenshots);
                    }
                    cx.notify();
                })),
        );
    }
    list
}
