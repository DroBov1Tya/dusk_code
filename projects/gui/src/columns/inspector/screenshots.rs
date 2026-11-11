//! Screenshots the model took of the screen, newest first, as small tiles; a click opens
//! one large over the window.

use gpui_kit::{component::ActiveTheme, *};

use crate::{modal::Modal, pages::chat::messages::picture_path, ui::widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let shots = ws
        .shown_chat()
        .map(|chat| chat.screenshots.as_slice())
        .unwrap_or_default();
    if shots.is_empty() {
        return div().child(widgets::muted("Screenshots the model takes show here.", cx));
    }
    let surface = cx.theme().muted;
    let mut grid = div().flex().flex_row().flex_wrap().gap_2().px_4();
    for (index, shot) in shots.iter().rev().enumerate() {
        let Some(path) = picture_path(&ws.status.artifacts_dir, &shot.id) else {
            continue;
        };
        let large = path.clone();
        grid = grid.child(
            div()
                .id(("screenshot", index))
                .w(px(148.))
                .h(px(96.))
                .rounded_md()
                .overflow_hidden()
                .bg(surface)
                .cursor_pointer()
                .child(img(path).size_full().object_fit(ObjectFit::Cover))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(Modal::Picture {
                        path: large.clone(),
                    });
                    cx.notify();
                })),
        );
    }
    grid
}
