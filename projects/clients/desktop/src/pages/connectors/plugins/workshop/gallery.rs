//! A listing's screenshots, as a store page has them: the one in view large on a stage
//! at 16:10, fitted whole (a shot of another shape is letterboxed, never cropped or
//! stretched), arrows on it, "n of N" and its caption under it, then a strip of every
//! shot that scrolls with the wheel. Only screenshots: the cover is in the window's
//! header. Left and Right step while the window is open.

use std::{sync::Arc, time::Duration};

use dusk_protocol::plugin::Listing;
use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use super::{capsule, covers::Cover, hero};
use crate::{
    modal::Modal,
    ui::{Type, size},
    workspace::Workspace,
};

/// The stage's and the thumbs' width over their height.
const ASPECT: f32 = 16. / 10.;
/// The width of a shot in the strip.
const THUMB: f32 = 112.;
const RADIUS: f32 = 8.;
const FADE: Duration = Duration::from_millis(240);
/// A caption is one line; the rest is never shaped.
const CAPTION_CHARS: usize = 200;

/// One shot: none while it comes.
struct Shot {
    image: Option<Arc<Image>>,
    caption: Option<String>,
}

/// The screenshots that have come or are coming; a failed one is left out.
fn shots(ws: &Workspace, listing: &Listing) -> Vec<Shot> {
    ws.workshop
        .shots
        .get(listing)
        .iter()
        .zip(&listing.screenshots)
        .filter_map(|(shot, about)| {
            let image = match shot {
                Cover::Ready(image) => Some(Arc::clone(image)),
                Cover::Loading => None,
                Cover::Failed => return None,
            };
            Some(Shot {
                image,
                caption: about.caption.clone(),
            })
        })
        .collect()
}

/// The gallery at `width`, or nothing when the listing has no screenshots to show.
pub fn render(
    ws: &Workspace,
    listing: &Listing,
    width: f32,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let theme = cx.theme();
    let (stage_back, border, ring, muted) = (
        theme.sidebar,
        theme.border,
        theme.primary,
        theme.muted_foreground,
    );
    let shots = shots(ws, listing);
    let count = shots.len();
    if count == 0 {
        return None;
    }
    let index = ws.workshop.gallery.min(count - 1);
    let height = (width / ASPECT).round();
    let mut stage = div()
        .relative()
        .w(px(width))
        .h(px(height))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(border)
        .bg(stage_back)
        .child(picture(shots[index].image.clone()).with_animation(
            ("gallery", index),
            Animation::new(FADE),
            // It starts part shown, so a frame drawn at once never shows an empty stage.
            |picture, delta| picture.opacity(0.35 + 0.65 * delta),
        ));
    if count > 1 {
        let top = (height - size::CONTROL) / 2.;
        stage = stage
            .child(
                hero::round("gallery-back", IconName::ChevronLeft, cx)
                    .top(px(top))
                    .left(px(size::GAP_M))
                    .on_click(cx.listener(|ws, _, _, cx| ws.gallery_step(-1, cx))),
            )
            .child(
                hero::round("gallery-next", IconName::ChevronRight, cx)
                    .top(px(top))
                    .right(px(size::GAP_M))
                    .on_click(cx.listener(|ws, _, _, cx| ws.gallery_step(1, cx))),
            );
    }
    let mut line = format!("{} of {count}", index + 1);
    if let Some(caption) = &shots[index].caption {
        line = format!("{line} \u{b7} {}", capsule::clip(caption, CAPTION_CHARS));
    }
    let thumbs = shots
        .into_iter()
        .enumerate()
        .map(|(each, Shot { image, .. })| {
            let chosen = each == index;
            div()
                .id(("gallery-thumb", each))
                .flex_none()
                .w(px(THUMB))
                .h(px((THUMB / ASPECT).round()))
                .rounded(px(RADIUS))
                .border_2()
                .border_color(if chosen { ring } else { border })
                .bg(stage_back)
                .opacity(if chosen { 1. } else { 0.6 })
                .hover(|thumb| thumb.opacity(1.))
                .cursor_pointer()
                .child(picture(image))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.gallery = each;
                    cx.notify();
                }))
        });
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .child(stage)
        .child(div().small().text_color(muted).truncate().child(line));
    if count > 1 {
        column = column.child(
            div()
                .id("gallery-strip")
                .flex()
                .flex_row()
                .gap(px(size::GAP_S))
                .overflow_x_scroll()
                .track_scroll(&ws.workshop.strip)
                .children(thumbs),
        );
    }
    Some(column)
}

/// A shot fitted whole in its box; the empty box while it comes.
fn picture(image: Option<Arc<Image>>) -> Div {
    let frame = div().size_full().p(px(1.));
    match image {
        Some(image) => frame.child(
            img(image)
                .size_full()
                .rounded(px(RADIUS - 1.))
                .object_fit(ObjectFit::Contain),
        ),
        None => frame,
    }
}

impl Workspace {
    /// Steps the open listing's gallery, keeping the strip on the shot in view.
    pub fn gallery_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(Modal::Listing { source, name }) = &self.modal else {
            return;
        };
        let Some(listing) = super::find(self, source, name) else {
            return;
        };
        let count = shots(self, listing).len();
        if count == 0 {
            return;
        }
        let shop = &mut self.workshop;
        let next = shop.gallery as isize + delta;
        shop.gallery = next.clamp(0, count as isize - 1) as usize;
        shop.strip.scroll_to_item(shop.gallery);
        cx.notify();
    }
}
