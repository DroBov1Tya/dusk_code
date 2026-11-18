//! The pictures a user's message carries, as thumbnails under its words. The bytes come
//! from the core by the source's name, never from a path; a click opens one large.

use crate::ui::elevation::{Elevation, Level};
use std::sync::Arc;

use gpui_kit::{component::ActiveTheme, *};

use crate::{ui::Type, workspace::Workspace};

/// A thumbnail keeps its size before its picture comes, so the list's cached row height
/// stays right.
const WIDTH: f32 = 168.;
const HEIGHT: f32 = 120.;

/// A picture asked of the core.
pub enum SentPicture {
    Loading,
    Ready(Arc<Image>),
    /// Removed from the sources, or not a picture after all.
    Gone,
}

/// The sources a message names whose names look like pictures; the core checks the bytes.
pub fn picture_names(text: &str) -> Vec<String> {
    use dusk_protocol::thread::{Part, split};
    split(text)
        .into_iter()
        .filter_map(|part| match part {
            Part::Source(name) if is_picture_name(name) => Some(name.to_owned()),
            _ => None,
        })
        .collect()
}

pub fn is_picture_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

impl Workspace {
    /// Asks the core once for each picture of a message not yet asked for.
    pub fn load_sent_pictures(&mut self, thread_id: &str, text: &str, cx: &mut Context<Self>) {
        for name in picture_names(text) {
            let slot = (thread_id.to_owned(), name.clone());
            if self.sent_pictures.contains_key(&slot) {
                continue;
            }
            self.sent_pictures
                .insert(slot.clone(), SentPicture::Loading);
            let thread_id = thread_id.to_owned();
            self.core.call(
                cx,
                move |client| async move { client.work().source_picture(&thread_id, &name).await },
                move |ws, result, cx| {
                    let picture = match result.map(|bytes| (format(&bytes), bytes)) {
                        Ok((Some(format), bytes)) => {
                            SentPicture::Ready(Arc::new(Image::from_bytes(format, bytes)))
                        }
                        Ok((None, _)) => SentPicture::Gone,
                        Err(error) => {
                            tracing::warn!("cannot load a sent picture: {error:#}");
                            SentPicture::Gone
                        }
                    };
                    ws.sent_pictures.insert(slot, picture);
                    cx.notify();
                },
            );
        }
    }
}

/// The thumbnails of a message's pictures, in a row at the right like its words.
pub fn sent_pictures(
    ws: &Workspace,
    thread_id: &str,
    key: &str,
    text: &str,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let names = picture_names(text);
    if names.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let tiles = names.into_iter().enumerate().map(|(n, name)| {
        let tile = div()
            .id(SharedString::from(format!("{key}-picture-{n}")))
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .flex_none()
            .rounded_lg()
            .overflow_hidden()
            .elevation(Level::Raised, cx);
        match ws.sent_pictures.get(&(thread_id.to_owned(), name.clone())) {
            Some(SentPicture::Ready(image)) => {
                let large = image.clone();
                tile.cursor_pointer()
                    .hover(|tile| tile.opacity(0.9))
                    .active(|tile| tile.opacity(0.8))
                    .child(
                        img(image.clone())
                            .size_full()
                            .rounded_lg()
                            .object_fit(ObjectFit::Cover),
                    )
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.open_picture(large.clone().into(), window, cx);
                    }))
            }
            Some(SentPicture::Gone) => tile
                .p_2()
                .flex()
                .items_center()
                .justify_center()
                .meta()
                .text_color(muted)
                .child(div().truncate().child(name)),
            Some(SentPicture::Loading) | None => tile,
        }
    });
    Some(
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .justify_end()
            .gap_2()
            .children(tiles),
    )
}
