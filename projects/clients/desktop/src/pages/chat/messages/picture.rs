//! A picture the agent showed, as a card from the pictures folder.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

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

/// The widest and the tallest a card's picture gets, in points.
const MAX_WIDTH: f32 = 220.;
const MAX_HEIGHT: f32 = 320.;
/// The box of a picture whose size cannot be read.
const FALLBACK: (f32, f32) = (MAX_WIDTH, 160.);

/// The pixel size of a picture, read from its header once. A picture's file never
/// changes under its ID, so the answer is kept.
fn natural_size(path: &Path) -> Option<(u32, u32)> {
    static SIZES: OnceLock<Mutex<HashMap<PathBuf, (u32, u32)>>> = OnceLock::new();
    let mut sizes = SIZES.get_or_init(Default::default).lock().ok()?;
    if let Some(size) = sizes.get(path) {
        return Some(*size);
    }
    let size = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .ok()?
        .into_dimensions()
        .ok()?;
    sizes.insert(path.to_owned(), size);
    Some(size)
}

/// The size a picture is drawn at: its own, scaled down to fit the bounds with its
/// aspect ratio, never up. `gpui`'s `img` with a percent width takes the natural height,
/// which stretched wide pictures and cropped tall ones.
fn fitted(width: u32, height: u32) -> (f32, f32) {
    let (width, height) = (width.max(1) as f32, height.max(1) as f32);
    let scale = (MAX_WIDTH / width).min(MAX_HEIGHT / height).min(1.);
    ((width * scale).max(1.), (height * scale).max(1.))
}

/// A picture from the pictures folder as a card: the picture on the left, its title and
/// description beside it, a hairline under it to part it from the next. Only an ID of
/// hex digits and dashes is taken, so it cannot name a path outside that folder.
pub fn picture(folder: &Path, image: &Picture, cx: &mut Context<Workspace>) -> AnyElement {
    let id = &image.id;
    let Some(path) = picture_path(folder, id) else {
        return div().into_any_element();
    };
    let (width, height) = natural_size(&path).map_or(FALLBACK, |(w, h)| fitted(w, h));
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
        // A fixed column keeps the text aligned whatever size the picture is.
        .child(
            div().w(px(MAX_WIDTH)).flex_none().child(
                div()
                    .id(SharedString::from(format!("picture-card-{id}")))
                    .w(px(width))
                    .h(px(height))
                    .flex_none()
                    .rounded_lg()
                    .overflow_hidden()
                    .bg(surface)
                    .cursor_pointer()
                    .child(img(path.clone()).size_full().object_fit(ObjectFit::Contain))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.modal = Some(crate::modal::Modal::Picture {
                            image: path.clone().into(),
                        });
                        cx.notify();
                    })),
            ),
        )
        .child(text)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::fitted;

    #[test]
    fn a_picture_is_scaled_down_never_up_and_keeps_its_ratio() {
        assert_eq!(fitted(110, 40), (110., 40.));
        assert_eq!(fitted(4400, 200), (220., 10.));
        assert_eq!(fitted(100, 1280), (25., 320.));
    }
}
