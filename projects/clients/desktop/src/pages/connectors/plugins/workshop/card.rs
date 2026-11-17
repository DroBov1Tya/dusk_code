//! One listing of the store as a card: its cover, name and version, its tier mark, kind
//! and stars, what it is about, and the buttons that install, update or remove it. A
//! pack's card opens the pack's page.

use dusk_protocol::plugin::{Client, Kind, Listing};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Confirm, tier};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub const WIDTH: f32 = 340.;

pub fn render(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (border, muted, warning, danger, surface) = (
        theme.border,
        theme.muted_foreground,
        theme.warning,
        theme.danger,
        theme.muted,
    );
    let name = &listing.name;
    let busy = ws.workshop.busy.get(name).copied();
    let failed = ws.workshop.failed.get(name).cloned();
    let headless = !listing.clients.is_empty() && !listing.clients.contains(&Client::Gui);
    let notes = [
        listing
            .clash
            .then(|| ("Another source offers the same name".into(), warning)),
        headless.then(|| ("Its panels do not show in this app".into(), muted)),
        listing
            .unfit
            .clone()
            .map(|why| (SharedString::from(why), warning)),
        failed.map(|error| (SharedString::from(error), danger)),
    ];
    let pack = listing.kind == Kind::Pack;
    let opened = name.clone();
    div()
        .id(id("card", listing))
        .w(px(WIDTH))
        .max_w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .when(pack, |card| {
            card.cursor_pointer()
                .hover(|card| card.border_color(muted.opacity(0.4)))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.pack = Some(opened.clone());
                    cx.notify();
                }))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(thumb(ws, listing, 44., cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .body()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(name.clone()),
                                )
                                .child(
                                    div()
                                        .meta()
                                        .text_color(muted)
                                        .child(listing.version.clone()),
                                ),
                        )
                        .child(facts(listing, cx)),
                ),
        )
        .child(
            div()
                .small()
                .text_color(muted)
                .line_clamp(2)
                .text_ellipsis()
                .min_h(px(34.))
                .child(listing.description.clone()),
        )
        .children(
            notes
                .into_iter()
                .flatten()
                .map(|(text, color)| div().meta().text_color(color).child(text)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(div().flex_1())
                .children(homepage(listing))
                // The buttons keep their clicks, so Install does not open a pack's page.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(actions(listing, busy, cx)),
                ),
        )
}

/// The tier's mark, what kind it is and its stars, on one line.
pub fn facts(listing: &Listing, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(tier::mark(listing.tier, cx))
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(super::kind_word(listing.kind)),
        );
    if listing.stars > 0 {
        row = row.child(stars(listing.stars, cx));
    }
    if listing.kind == Kind::Pack && !listing.skills.is_empty() {
        row = row.child(
            div()
                .meta()
                .text_color(muted)
                .child(widgets::count(listing.skills.len(), "skill")),
        );
    }
    row
}

pub fn stars(stars: u32, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_0p5()
        .meta()
        .text_color(muted)
        .child(Icon::new(IconName::Star).size(px(12.)).text_color(muted))
        .child(stars.to_string())
}

/// The cover, or a letter tile until it comes or when there is none.
pub fn thumb(ws: &Workspace, listing: &Listing, size: f32, cx: &App) -> Div {
    match ws.workshop.covers.get(listing) {
        Some(image) => div()
            .size(px(size))
            .flex_none()
            .rounded_lg()
            .overflow_hidden()
            .child(img(image).size_full().object_fit(ObjectFit::Cover)),
        None => crate::pages::connectors::card::icon_slot(&listing.name, cx).size(px(size)),
    }
}

fn homepage(listing: &Listing) -> Option<Button> {
    let url = listing
        .homepage
        .clone()
        .filter(|url| url.starts_with("https://"))?;
    Some(
        Button::new(id("home", listing))
            .ghost()
            .small()
            .icon(Icon::new(IconName::ExternalLink))
            .tooltip("Open its page in the browser")
            .on_click(move |_, _, cx| cx.open_url(&url)),
    )
}

/// One at work; Install; Update with Remove; or Installed with Remove.
pub fn actions(
    listing: &Listing,
    busy: Option<&'static str>,
    cx: &mut Context<Workspace>,
) -> Vec<Button> {
    if let Some(doing) = busy {
        return vec![
            Button::new(id("busy", listing))
                .small()
                .loading(true)
                .label(doing)
                .disabled(true),
        ];
    }
    let remove =
        {
            let name = listing.name.clone();
            Button::new(id("remove", listing))
                .ghost()
                .small()
                .label("Remove")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.confirm_store(Confirm::Remove(name.clone()), cx)
                }))
        };
    let install = |label: String, update: bool| {
        let shown = listing.clone();
        let button = Button::new(id("install", listing)).small();
        // An update is the one thing on the page that waits on the user.
        let button = if update {
            button.primary()
        } else {
            button.outline()
        };
        button
            .label(label)
            .disabled(listing.unfit.is_some())
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.confirm_store(Confirm::Install(Box::new(shown.clone())), cx)
            }))
    };
    match (&listing.installed, &listing.update) {
        (None, _) => vec![install("Install".into(), false)],
        (Some(_), Some(version)) => vec![remove, install(format!("Update to {version}"), true)],
        (Some(_), None) => vec![
            remove,
            Button::new(id("installed", listing))
                .small()
                .ghost()
                .label("Installed")
                .disabled(true),
        ],
    }
}

/// Names repeat across sources, so an element's id carries both.
pub fn id(what: &str, listing: &Listing) -> SharedString {
    SharedString::from(format!("store-{what}-{}-{}", listing.source, listing.name))
}
