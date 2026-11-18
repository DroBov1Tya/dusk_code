//! What a listing's slot holds and its buttons do, the same on a card, the hero, a pack's
//! page and a listing's window: one thing at a time, a button or a state, never both.
//! Install, Retry after a failure, Update (the one filled button: it waits on the user),
//! a turning ring while at work, or "Installed"; Remove only where there is room for it.

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

/// The narrowest an action button is, so a row of cards keeps its buttons one width.
pub const WIDTH: f32 = 88.;
/// A button in a card's footer is a little lower than a control.
const IN_CARD: f32 = 28.;
const STATE_ICON: f32 = 14.;

/// The slot of a card (`compact`: lower buttons, "Update" without its version) or of
/// the hero.
pub fn slot(
    listing: &Listing,
    ws: &Workspace,
    compact: bool,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let height = if compact { IN_CARD } else { size::CONTROL };
    if let Some(doing) = ws.workshop.busy.get(&listing.name).copied() {
        return busy(listing, doing).h(px(height)).into_any_element();
    }
    let failed = ws.workshop.failed.contains_key(&listing.name);
    let button = match (&listing.installed, &listing.update) {
        (None, _) if failed => install(listing, "Retry".into(), false, cx),
        (None, _) => install(listing, "Install".into(), false, cx),
        (Some(_), Some(version)) => {
            let label = if compact {
                "Update".to_owned()
            } else {
                format!("Update to {version}")
            };
            install(listing, label, true, cx)
        }
        (Some(_), None) => return installed(cx).into_any_element(),
    };
    button.h(px(height)).into_any_element()
}

/// Remove, once installed and not at work: danger-coloured and set apart from the
/// primary action, never first; it asks before anything is deleted.
pub fn remove(listing: &Listing, ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Button> {
    if listing.installed.is_none() || ws.workshop.busy.contains_key(&listing.name) {
        return None;
    }
    let name = listing.name.clone();
    Some(
        Button::new(id("remove", listing))
            .danger()
            .outline()
            .min_w(px(WIDTH))
            .label("Remove")
            .on_click(cx.listener(move |ws, _, window, cx| {
                cx.stop_propagation();
                ws.confirm_store(Confirm::Remove(name.clone()), window, cx)
            })),
    )
}

/// "Installed" with its check, in the success colour: a state, not a button.
pub fn installed(cx: &App) -> Div {
    state(IconName::Check, "Installed", cx.theme().success)
}

/// A glyph and a word in one colour: a state of a listing.
pub fn state(icon: IconName, word: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .meta()
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(Icon::new(icon).size(px(STATE_ICON)).text_color(color))
        .child(word.into())
}

pub fn busy(listing: &Listing, doing: &'static str) -> Button {
    Button::new(id("busy", listing))
        .ghost()
        .min_w(px(WIDTH))
        // The kit turns the ring only in the place of an icon.
        .icon(Icon::new(IconName::LoaderCircle))
        .loading(true)
        .label(doing)
        .disabled(true)
}

/// Install, Retry or Update: it asks first, and it is off where the listing cannot run.
pub fn install(
    listing: &Listing,
    label: String,
    filled: bool,
    cx: &mut Context<Workspace>,
) -> Button {
    let shown = listing.clone();
    let button = Button::new(id("install", listing)).min_w(px(WIDTH));
    let button = if filled {
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
        .on_click(cx.listener(move |ws, _, window, cx| {
            cx.stop_propagation();
            ws.confirm_store(Confirm::Install(Box::new(shown.clone())), window, cx)
        }))
}

/// Names repeat across sources, so an element's id carries both.
pub fn id(what: &str, listing: &Listing) -> SharedString {
    SharedString::from(format!("store-{what}-{}-{}", listing.source, listing.name))
}
