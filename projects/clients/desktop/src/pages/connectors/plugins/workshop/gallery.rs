//! A listing's gallery, as a store page has: the picture in view large at the art's
//! aspect (a screenshot of another shape is fitted whole, never stretched), arrows on it
//! and its caption under it, then a strip of every picture that scrolls with its arrows
//! and the wheel. The cover comes first, then the screenshots; Left and Right step while
//! the window is open.

use std::{sync::Arc, time::Duration};

use dusk_protocol::plugin::Listing;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{capsule, covers::Cover, grid, hero};
use crate::{
    modal::Modal,
    ui::{Type, size},
    workspace::Workspace,
};

/// The width of a picture in the strip.
const THUMB: f32 = 112.;
const FADE: Duration = Duration::from_millis(240);

/// One picture of the gallery: none while it comes or when there is no cover.
struct Media {
    image: Option<Arc<Image>>,
    caption: Option<String>,
}

/// The cover, then the screenshots that have come or are coming; a failed one is left out.
fn media(ws: &Workspace, listing: &Listing) -> Vec<Media> {
    let cover = Media {
        image: ws.workshop.covers.get(listing),
        caption: None,
    };
    let shots = ws
        .workshop
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
            Some(Media {
                image,
                caption: about.caption.clone(),
            })
        });
    std::iter::once(cover).chain(shots).collect()
}

pub fn render(ws: &Workspace, listing: &Listing, width: f32, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (back, border, ring) = (theme.background, theme.border, theme.primary);
    let media = media(ws, listing);
    let count = media.len();
    let index = ws.workshop.gallery.min(count - 1);
    let height = grid::cover_height(width);
    let picture = |image: Option<Arc<Image>>, width: f32| match image {
        Some(image) => div()
            .w(px(width))
            .h(px(grid::cover_height(width)))
            .child(img(image).size_full().object_fit(ObjectFit::Contain)),
        None => capsule::art(None, &listing.name, listing.tier, width, None, cx),
    };
    let mut stage = div()
        .relative()
        .w(px(width))
        .h(px(height))
        .rounded_lg()
        .overflow_hidden()
        .bg(back)
        .child(picture(media[index].image.clone(), width).with_animation(
            ("gallery", index),
            Animation::new(FADE),
            |picture, delta| picture.opacity(delta),
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
    let caption = media[index].caption.clone();
    let thumbs = media
        .into_iter()
        .enumerate()
        .map(|(each, Media { image, .. })| {
            let chosen = each == index;
            div()
                .id(("gallery-thumb", each))
                .flex_none()
                .rounded_md()
                .overflow_hidden()
                .border_2()
                .border_color(if chosen { ring } else { border })
                .opacity(if chosen { 1. } else { 0.6 })
                .hover(|thumb| thumb.opacity(1.))
                .cursor_pointer()
                .child(picture(image, THUMB))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.gallery = each;
                    cx.notify();
                }))
        });
    let step = |id: &'static str, icon: IconName, delta: isize, off: bool| {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .disabled(off)
            .on_click(cx.listener(move |ws, _, _, cx| ws.gallery_step(delta, cx)))
    };
    let strip = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .child(step("strip-back", IconName::ChevronLeft, -1, index == 0))
        .child(
            div()
                .id("gallery-strip")
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .gap(px(size::GAP_S))
                .overflow_x_scroll()
                .track_scroll(&ws.workshop.strip)
                .children(thumbs),
        )
        .child(step(
            "strip-next",
            IconName::ChevronRight,
            1,
            index + 1 >= count,
        ));
    let mut column = div().flex().flex_col().gap(px(size::GAP_M)).child(stage);
    if let Some(caption) = caption {
        column = column.child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child(caption),
        );
    }
    if count > 1 {
        column = column.child(strip);
    }
    column
}

impl Workspace {
    /// Steps the open listing's gallery, keeping the strip on the picture in view.
    pub fn gallery_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(Modal::Listing { source, name }) = &self.modal else {
            return;
        };
        let Some(listing) = super::find(self, source, name) else {
            return;
        };
        let count = media(self, listing).len();
        let shop = &mut self.workshop;
        let next = shop.gallery as isize + delta;
        shop.gallery = next.clamp(0, count as isize - 1) as usize;
        shop.strip.scroll_to_item(shop.gallery);
        cx.notify();
    }
}
