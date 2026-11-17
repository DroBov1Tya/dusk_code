//! A listing's window, as a store page: the gallery and the whole description on the
//! left; on the right its name, where it comes from, its facts and tags, its buttons at
//! a fixed place under them, what it warns of, what it may do, its page and its dates.
//! A pack lists its skills and leads to its page.

use dusk_protocol::plugin::{Client, Kind, Listing};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{actions, capsule, gallery, hero};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The width of the right column.
const SIDE: f32 = 296.;
/// A narrower window puts the right column under the gallery.
const SIDE_BY_SIDE: f32 = 760.;
/// The window's width at most.
pub const WIDTH: f32 = 1040.;

pub fn render(
    ws: &Workspace,
    source: &str,
    name: &str,
    width: f32,
    cx: &mut Context<Workspace>,
) -> Div {
    let page = div().flex().p(px(size::GAP_XL)).gap(px(size::GAP_XL));
    let Some(listing) = super::find(ws, source, name) else {
        return page.child(widgets::muted("This listing is no longer offered.", cx));
    };
    let inner = width - 2. * size::GAP_XL - 2.;
    let wide = width >= SIDE_BY_SIDE;
    let main_w = if wide {
        inner - SIDE - size::GAP_XL
    } else {
        inner
    };
    let main = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XL))
        .w(px(main_w))
        .flex_none()
        .child(gallery::render(ws, listing, main_w, cx))
        .child(about(listing, cx));
    let side = side(ws, listing, cx).w(px(if wide { SIDE } else { main_w }));
    if wide {
        page.flex_row().items_start().child(main).child(side)
    } else {
        page.flex_col().child(main).child(side)
    }
}

/// The full description under the gallery.
fn about(listing: &Listing, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child("About"),
        )
        .child(
            div()
                .body()
                .text_color(cx.theme().foreground.opacity(0.86))
                .child(listing.description.clone()),
        )
}

fn side(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, warning, danger) = (theme.muted_foreground, theme.warning, theme.danger);
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let mut column = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(size::GAP_L))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(size::GAP_S))
                .child(div().title().child(listing.name.clone()))
                .child(
                    div()
                        .meta()
                        .text_color(muted)
                        .child(format!("From {source}")),
                )
                .child(hero::facts(listing, cx)),
        );
    if let Some(tags) = hero::tags(listing, cx) {
        column = column.child(tags);
    }
    let mut buttons = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(size::GAP_S))
        .min_h(px(size::CONTROL))
        .children(actions::all(listing, ws, cx));
    if listing.installed.is_some() && listing.update.is_none() {
        buttons = buttons.children(capsule::state_badge(listing, cx));
    }
    column = column.child(buttons);
    let headless = !listing.clients.is_empty() && !listing.clients.contains(&Client::Gui);
    let notes = [
        listing.clash.then(|| {
            (
                "Another source offers the same name; check where it comes from.".to_owned(),
                warning,
            )
        }),
        listing.unfit.clone().map(|why| (why, warning)),
        headless.then(|| ("Its panels do not show in this app.".to_owned(), muted)),
        ws.workshop
            .failed
            .get(&listing.name)
            .map(|error| (error.clone(), danger)),
    ];
    for (text, color) in notes.into_iter().flatten() {
        column = column.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(size::GAP_S))
                .small()
                .text_color(color)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(px(size::ICON))
                        .text_color(color),
                )
                .child(div().flex_1().min_w_0().child(text)),
        );
    }
    if listing.kind == Kind::Plugin {
        column = column.child(list(
            "It may",
            match listing.permissions.is_empty() {
                true => vec!["Nothing beyond its own folder".to_owned()],
                false => listing.permissions.clone(),
            },
            cx,
        ));
    }
    if listing.kind == Kind::Pack {
        let opened = listing.name.clone();
        column = column
            .child(list(
                &widgets::count(listing.skills.len(), "skill"),
                listing.skills.clone(),
                cx,
            ))
            .child(
                div().flex().child(
                    Button::new("detail-pack-page")
                        .link()
                        .label("Open the pack's page")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.workshop.pack = Some(opened.clone());
                            ws.close_modal(cx);
                        })),
                ),
            );
    }
    let mut facts = Vec::new();
    if let Some(date) = day(&listing.created) {
        facts.push(("Released", date));
    }
    if let Some(date) = day(&listing.updated) {
        facts.push(("Updated", date));
    }
    if let Some(version) = &listing.installed {
        facts.push(("Installed", version.clone()));
    }
    if !facts.is_empty() {
        column = column.child(div().flex().flex_col().gap(px(size::GAP_XS)).children(
            facts.into_iter().map(|(label, value)| {
                div()
                    .flex()
                    .flex_row()
                    .small()
                    .child(div().w(px(SIDE / 3.)).text_color(muted).child(label))
                    .child(value)
            }),
        ));
    }
    if let Some(url) = listing
        .homepage
        .clone()
        .filter(|url| url.starts_with("https://"))
    {
        column = column.child(
            div().flex().child(
                Button::new("detail-homepage")
                    .link()
                    .icon(Icon::new(IconName::ExternalLink))
                    .label("Its page")
                    .tooltip(url.clone())
                    .on_click(move |_, _, cx| cx.open_url(&url)),
            ),
        );
    }
    column
}

/// A titled list of plain lines.
fn list(title: &str, lines: Vec<String>, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XS))
        .child(div().group_name().text_color(muted).child(title.to_owned()))
        .children(lines.into_iter().map(|line| div().small().child(line)))
}

/// The day of an RFC 3339 time, as it is written there.
fn day(text: &Option<String>) -> Option<String> {
    let text = text.as_deref()?;
    let day = text.get(..10)?;
    day.chars()
        .all(|c| c.is_ascii_digit() || c == '-')
        .then(|| day.to_owned())
}
