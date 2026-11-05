//! Markdown as the chat draws it: links open in the system browser and pictures load,
//! both only over http(s), so a message cannot reach a local file or another scheme.

use gpui_kit::{component::text::TextView, *};

pub fn markdown(id: impl Into<ElementId>, text: String) -> TextView {
    TextView::markdown(id, text)
        .selectable(true)
        .on_link_click(|url, _, _, cx| {
            if is_web(url) {
                cx.open_url(url);
            } else {
                tracing::warn!("a link that is not http(s) was not opened");
            }
        })
        .image_source(|uri| {
            if is_web(uri.as_ref()) {
                ImageSource::from(uri.clone())
            } else {
                // An empty source draws nothing instead of reading the disk.
                ImageSource::from(SharedUri::from(""))
            }
        })
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}
