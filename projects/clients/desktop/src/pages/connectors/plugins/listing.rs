//! One plugin of the store as a row: its name and version, what it is, where it runs,
//! and the one button that installs, updates or removes it.

use dusk_protocol::plugin::{Client, Listing, Os};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::Confirm;
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(
    ws: &Workspace,
    listing: &Listing,
    divided: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (border, muted, warning, danger) = (
        theme.border,
        theme.muted_foreground,
        theme.warning,
        theme.danger,
    );
    let name = &listing.name;
    let busy = ws.workshop.busy.get(name).copied();
    let failed = ws.workshop.failed.get(name).cloned();
    let platforms = names(listing.platforms.iter().map(|os| os_name(*os)));
    let clients = names(listing.clients.iter().map(|client| client_name(*client)));
    let headless = !listing.clients.is_empty() && !listing.clients.contains(&Client::Gui);
    let homepage = listing
        .homepage
        .clone()
        .filter(|url| url.starts_with("https://"));
    let heading = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline()
        .gap_2()
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.clone()),
        )
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(listing.version.clone()),
        )
        .children(platforms.map(|text| widgets::badge(text, muted)))
        .children(clients.map(|text| widgets::badge(text, muted)));
    let notes = [
        headless.then(|| {
            (
                SharedString::from("Its panels do not show in this app"),
                muted,
            )
        }),
        listing
            .unfit
            .clone()
            .map(|why| (SharedString::from(why), warning)),
        failed.map(|error| (SharedString::from(error), danger)),
    ];
    div()
        .flex()
        .flex_row()
        .gap_3()
        .px_4()
        .py_3()
        .when(divided, |row| row.border_t_1().border_color(border))
        .child(super::card::icon_slot(name, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(heading)
                .child(
                    div()
                        .small()
                        .text_color(muted)
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(listing.description.clone()),
                )
                .children(
                    notes
                        .into_iter()
                        .flatten()
                        .map(|(text, color)| div().meta().text_color(color).child(text)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_start()
                .gap_1()
                .children(homepage.map(|url| {
                    Button::new(SharedString::from(format!("store-home-{name}")))
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::ExternalLink))
                        .tooltip("Open its page in the browser")
                        .on_click(move |_, _, cx| cx.open_url(&url))
                }))
                .children(actions(listing, busy, cx)),
        )
}

/// The buttons of a row: one at work, Install or Update, or Installed with Remove.
fn actions(
    listing: &Listing,
    busy: Option<&'static str>,
    cx: &mut Context<Workspace>,
) -> Vec<Button> {
    let id = |what: &str| SharedString::from(format!("store-{what}-{}", listing.name));
    if let Some(doing) = busy {
        return vec![
            Button::new(id("busy"))
                .small()
                .loading(true)
                .label(doing)
                .disabled(true),
        ];
    }
    let remove =
        {
            let name = listing.name.clone();
            Button::new(id("remove"))
                .ghost()
                .small()
                .label("Remove")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.confirm_store(Confirm::Remove(name.clone()), cx)
                }))
        };
    let install = |label: &'static str| {
        let shown = listing.clone();
        let button = Button::new(id("install")).small();
        // An update is the one thing on the page that waits on the user.
        let button = if label == "Update" {
            button.primary()
        } else {
            button.outline()
        };
        button
            .label(label)
            .disabled(listing.unfit.is_some())
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.confirm_store(Confirm::Install(shown.clone()), cx)
            }))
    };
    match listing.installed.as_deref() {
        None => vec![install("Install")],
        Some(version) if version != listing.version => vec![remove, install("Update")],
        Some(_) => vec![
            remove,
            Button::new(id("installed"))
                .small()
                .ghost()
                .label("Installed")
                .disabled(true),
        ],
    }
}

/// "macOS · Linux"; none when the list is empty, which means all.
fn names(mut names: impl Iterator<Item = &'static str>) -> Option<String> {
    let first = names.next()?;
    Some(names.fold(first.to_owned(), |all, name| format!("{all} · {name}")))
}

fn os_name(os: Os) -> &'static str {
    match os {
        Os::Macos => "macOS",
        Os::Linux => "Linux",
        Os::Windows => "Windows",
    }
}

fn client_name(client: Client) -> &'static str {
    match client {
        Client::Gui => "GUI",
        Client::Tui => "TUI",
        Client::Web => "Web",
    }
}
