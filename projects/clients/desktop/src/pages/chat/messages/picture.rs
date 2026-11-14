//! A picture the agent showed, as a card from the pictures folder.

use std::path::Path;

use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::{links, transcript::Picture},
    ui::Type,
    workspace::Workspace,
};

/// A picture of the pictures folder. Only an ID of hex digits and dashes is taken, so
/// it cannot name a path outside that folder.
pub fn picture_path(folder: &Path, id: &str) -> Option<std::path::PathBuf> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| folder.join(id).join("image"))
}

/// A picture from the pictures folder as a card: the picture on the left, its title and
/// description beside it, a hairline under it to part it from the next. Only an ID of
/// hex digits and dashes is taken, so it cannot name a path outside that folder.
pub fn picture(folder: &Path, image: &Picture, cx: &mut Context<Workspace>) -> AnyElement {
    let id = &image.id;
    let Some(path) = picture_path(folder, id) else {
        return div().into_any_element();
    };
    let theme = cx.theme();
    let (border, muted, surface) = (theme.border, theme.muted_foreground, theme.muted);
    let mut text = div().flex().flex_col().gap_2().flex_1().min_w_0();
    if let Some(title) = &image.title {
        text = text.child(
            div()
                .text_lg()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.clone()),
        );
    }
    if let Some(description) = &image.description {
        text = text.child(links::markdown(
            SharedString::from(format!("picture-{id}")),
            description.clone(),
        ));
    }
    if image.title.is_none() && image.description.is_none() {
        text = text.child(div().meta().text_color(muted).child("Picture"));
    }
    div()
        .flex()
        .flex_row()
        .gap_5()
        .pb_4()
        .border_b_1()
        .border_color(border)
        .child(
            div()
                .id(SharedString::from(format!("picture-card-{id}")))
                .w(px(220.))
                .flex_none()
                .rounded_xl()
                .overflow_hidden()
                .bg(surface)
                .cursor_pointer()
                .child(
                    img(path.clone())
                        .w_full()
                        .max_h(px(320.))
                        .object_fit(ObjectFit::Cover),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(crate::modal::Modal::Picture { path: path.clone() });
                    cx.notify();
                })),
        )
        .child(text)
        .into_any_element()
}
