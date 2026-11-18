//! What a listing's buttons do and say, the same on a capsule, the hero, a pack's page
//! and a listing's window: Install, Update or a turning ring while at work, and Remove
//! where there is room for it. Every button is a control's height; the one that waits on
//! the user (an update) is the only filled one.

use dusk_protocol::plugin::Listing;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Confirm;
use crate::{
    ui::{Type, size},
    workspace::Workspace,
};

/// The narrowest an action button is, so a row of capsules keeps its buttons one width.
pub const WIDTH: f32 = 88.;

/// The one button of a capsule or the hero: at work, Install, Update, or none once
/// installed (the corner badge says so).
pub fn primary(
    listing: &Listing,
    ws: &Workspace,
    short: bool,
    cx: &mut Context<Workspace>,
) -> Option<Button> {
    if let Some(doing) = ws.workshop.busy.get(&listing.name).copied() {
        return Some(busy(listing, doing));
    }
    match (&listing.installed, &listing.update) {
        (None, _) => Some(install(listing, "Install".into(), false, cx)),
        (Some(_), Some(version)) => {
            let label = if short {
                "Update".to_owned()
            } else {
                format!("Update to {version}")
            };
            Some(install(listing, label, true, cx))
        }
        (Some(_), None) => None,
    }
}

/// All the buttons a listing's window and a pack's page have: the primary one, or
/// "Installed" with Remove.
pub fn all(listing: &Listing, ws: &Workspace, cx: &mut Context<Workspace>) -> Vec<Button> {
    if let Some(doing) = ws.workshop.busy.get(&listing.name).copied() {
        return vec![busy(listing, doing)];
    }
    let mut buttons: Vec<Button> = primary(listing, ws, false, cx).into_iter().collect();
    if listing.installed.is_some() {
        let name = listing.name.clone();
        buttons.push(
            Button::new(id("remove", listing))
                .outline()
                .min_w(px(WIDTH))
                .label("Remove")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.confirm_store(Confirm::Remove(name.clone()), cx)
                })),
        );
    }
    buttons
}

fn busy(listing: &Listing, doing: &'static str) -> Button {
    Button::new(id("busy", listing))
        .min_w(px(WIDTH))
        .loading(true)
        .label(doing)
        .disabled(true)
}

fn install(listing: &Listing, label: String, update: bool, cx: &mut Context<Workspace>) -> Button {
    let shown = listing.clone();
    let button = Button::new(id("install", listing)).min_w(px(WIDTH));
    let button = if update {
        button.primary()
    } else {
        button.outline()
    };
    let button = match &listing.unfit {
        Some(why) => button.tooltip(why.clone()),
        None => button,
    };
    button
        .label(label)
        .disabled(listing.unfit.is_some())
        .on_click(cx.listener(move |ws, _, _, cx| {
            cx.stop_propagation();
            ws.confirm_store(Confirm::Install(Box::new(shown.clone())), cx)
        }))
}

/// The star and the count, quiet.
pub fn stars(stars: u32, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .meta()
        .text_color(muted)
        .child(
            Icon::new(IconName::Star)
                .size(px(size::ICON))
                .text_color(muted),
        )
        .child(stars.to_string())
}

/// Names repeat across sources, so an element's id carries both.
pub fn id(what: &str, listing: &Listing) -> SharedString {
    SharedString::from(format!("store-{what}-{}-{}", listing.source, listing.name))
}
