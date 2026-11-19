//! A listing's window, as a store page: on the left its cover with its name, byline, mark
//! and categories, then its tabs: Overview (screenshots and the whole description),
//! Contents (its files), for a pack Skills and for a plugin that brings MCP servers
//! Connectors; on the right its action (with Remove apart and in the danger
//! colour), what it warns of, what it may access, where it comes from and its details
//! with dates in words, its license and its addresses shown. A tab with nothing in it is
//! not drawn. All of it is the publisher's text, drawn as plain text.

use dusk_protocol::plugin::{Client, Kind, Listing};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{
    actions,
    byline::{dated_byline, day},
    capsule, connectors, contents, gallery, hero, pack, tier,
};
use crate::{
    ui::{Type, controls, size, widgets},
    workspace::Workspace,
};

/// The tab of a listing's window in view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Overview,
    Contents,
    Skills,
    Connectors,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Contents => "Contents",
            Self::Skills => "Skills",
            Self::Connectors => "Connectors",
        }
    }
}

/// The width of the right column.
const SIDE: f32 = 296.;
/// A narrower window puts the right column under the rest.
const SIDE_BY_SIDE: f32 = 760.;
/// The window's width at most.
pub const WIDTH: f32 = 1040.;
/// The cover in the header.
const COVER: f32 = 320.;
/// A narrower main column puts the header's text under the cover.
const HEADER_SIDE_BY_SIDE: f32 = 560.;
const RADIUS: f32 = 12.;
/// The page's install button is taller than a control: it is the one thing to do here.
const ACTION: f32 = 40.;
/// The longest description and line drawn; the rest is never shaped.
const ABOUT_CHARS: usize = 4000;
const LINE_CHARS: usize = 300;

pub fn render(
    ws: &Workspace,
    source: &str,
    name: &str,
    (width, window_height): (f32, f32),
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
    let mut main = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XL))
        .w(px(main_w))
        .flex_none()
        .child(header(ws, listing, main_w, cx));
    let shown = shown_tabs(listing);
    let tab = if shown.len() > 1 {
        main = main.child(tabs(listing, &shown, ws.workshop.tab, cx));
        Some(ws.workshop.tab).filter(|tab| shown.contains(tab))
    } else {
        None
    };
    let main = match tab.unwrap_or_default() {
        Tab::Overview => {
            let stage_room = stage_room(window_height, main_w >= HEADER_SIDE_BY_SIDE);
            if let Some(gallery) = gallery::render(ws, listing, main_w, stage_room, cx) {
                main = main.child(gallery);
            }
            main.child(about(listing, cx))
        }
        Tab::Contents => main.child(contents::render(ws, listing, main_w, cx)),
        Tab::Skills => main.child(skills(ws, listing, cx)),
        Tab::Connectors => main.child(connectors::render(listing, cx)),
    };
    let side = side(ws, listing, cx).w(px(if wide { SIDE } else { main_w }));
    if wide {
        page.flex_row().items_start().child(main).child(side)
    } else {
        page.flex_col().child(main).child(side)
    }
}

/// The tallest stage that leaves the gallery's caption whole above the window's fold, for
/// a window of this height: the body scrolls, but a line cut at the fold at rest reads as
/// broken. The window is 86% of the room under the header bar (about 73 px); its title
/// row takes 53.
fn stage_room(window_height: f32, header_side_by_side: bool) -> f32 {
    const BAR: f32 = 73.;
    const TITLE_ROW: f32 = 53.;
    const CAPTION: f32 = 18.;
    const MARGIN: f32 = 8.;
    // Under the cover the header's text takes about this much more.
    const TEXT_UNDER: f32 = 110.;
    let body = 0.86 * (window_height - BAR) - TITLE_ROW;
    let header = (super::grid::cover_height(COVER - 2.) + 2.)
        + if header_side_by_side { 0. } else { TEXT_UNDER };
    body - size::GAP_XL - header - size::GAP_XL - size::GAP_S - CAPTION - MARGIN
}

/// The tabs with something in them, Overview first.
fn shown_tabs(listing: &Listing) -> Vec<Tab> {
    let mut shown = vec![Tab::Overview];
    if contents::has_files(listing) {
        shown.push(Tab::Contents);
    }
    if matches!(listing.kind, Kind::Pack | Kind::Plugin) && !listing.skills.is_empty() {
        shown.push(Tab::Skills);
    }
    if !listing.connectors.is_empty() {
        shown.push(Tab::Connectors);
    }
    shown
}

/// "Overview | Contents | Skills | Connectors", the ones with something in them; Contents asks the
/// core for the files when it opens, not before, since that may fetch the package.
fn tabs(listing: &Listing, shown: &[Tab], open: Tab, cx: &mut Context<Workspace>) -> Div {
    let mut row = div().flex().flex_row().items_center().gap(px(size::GAP_S));
    for (index, this) in shown.iter().copied().enumerate() {
        if index > 0 {
            row = row.child(controls::pipe(cx));
        }
        let id = SharedString::from(format!("detail-tab-{}", this.label().to_lowercase()));
        let listing = listing.clone();
        row = row.child(
            controls::text_choice(id, this.label(), open == this, cx).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.workshop.tab = this;
                    if this == Tab::Contents {
                        ws.open_contents(&listing, cx);
                    }
                    cx.notify();
                },
            )),
        );
    }
    row
}

/// The skills of a pack or a plugin, with their switches and descriptions once it is
/// here, by name before; and for a pack the way to its page. A plugin's skills land as
/// the pack of its name, so the rows are the same.
fn skills(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let opened = listing.name.clone();
    let list = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_M))
        .child(pack::skill_list(ws, listing, cx));
    if listing.kind != Kind::Pack {
        return list;
    }
    list.child(
        div().flex().child(
            Button::new("detail-pack-page")
                .link()
                .label("Open the pack's page")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.pack = Some(opened.clone());
                    ws.close_modal(cx);
                })),
        ),
    )
}

/// The cover with the name, the byline, the mark and the chips beside it, or under it
/// when narrow.
fn header(ws: &Workspace, listing: &Listing, width: f32, cx: &App) -> Div {
    let cover = div()
        .flex_none()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(cx.theme().border)
        .child(capsule::art(
            ws.cover(listing),
            &listing.name,
            listing.kind,
            COVER - 2.,
            Corners::all(px(RADIUS - 1.)),
            cx,
        ));
    let mut text = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .title()
                .child(capsule::clip(&listing.name, LINE_CHARS)),
        )
        .child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child(dated_byline(listing)),
        )
        .child(marks(listing, cx));
    if let Some(tags) = hero::tags(listing, cx) {
        text = text.child(tags);
    }
    let row = div().flex().gap(px(size::GAP_L));
    if width >= HEADER_SIDE_BY_SIDE {
        row.flex_row().items_start().child(cover).child(text)
    } else {
        row.flex_col().child(cover).child(text)
    }
}

/// The tier's mark, what the listing is and, for a mirror the user added, its stars.
fn marks(listing: &Listing, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(size::GAP_M))
        .child(tier::mark(listing.tier, cx))
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(super::kind_word(listing.kind)),
        );
    if let Some(stars) = &listing.repo_stars {
        row = row.child(div().meta().text_color(muted).child(stars.label()));
    }
    row
}

/// The full description, its line breaks kept.
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
                .child(paragraphs(&listing.description, ABOUT_CHARS)),
        )
}

fn side(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, warning, danger) = (theme.muted_foreground, theme.warning, theme.danger);
    let mut column = div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(size::GAP_XL))
        .child(action(ws, listing, cx));
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
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(capsule::clip(&text, LINE_CHARS)),
                ),
        );
    }
    let access = match (listing.kind, listing.permissions.is_empty()) {
        (Kind::Plugin, true) => vec!["Nothing beyond its own folder".to_owned()],
        (Kind::Plugin, false) => listing.permissions.clone(),
        _ => vec!["Nothing: it is text the model reads; it runs nothing".to_owned()],
    };
    column = column.child(list("What it may access", access, cx));
    column
        .child(origin(listing, cx))
        .child(details(listing, cx))
}

/// The one thing to do (Install, Retry or Update, filled), or "Installed" with its
/// version; Remove under it, apart, in the danger colour.
fn action(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut block = div().flex().flex_col().gap(px(size::GAP_S));
    let failed = ws.workshop.failed.contains_key(&listing.name);
    let asks = match (
        ws.workshop.busy.get(&listing.name).copied(),
        &listing.installed,
        &listing.update,
    ) {
        (Some(doing), _, _) => {
            block = block.child(actions::busy(listing, doing).h(px(ACTION)).w_full());
            false
        }
        (None, None, _) => {
            let label = if failed { "Retry" } else { "Install" };
            block = block.child(
                actions::install(listing, label.into(), true, cx)
                    .h(px(ACTION))
                    .w_full(),
            );
            true
        }
        (None, Some(_), Some(version)) => {
            block = block.child(
                actions::install(listing, format!("Update to {version}"), true, cx)
                    .h(px(ACTION))
                    .w_full(),
            );
            true
        }
        (None, Some(version), None) => {
            block = block.child(actions::state(
                IconName::Check,
                format!("Installed {version}"),
                cx.theme().success,
            ));
            false
        }
    };
    if asks {
        block = block.child(
            div()
                .meta()
                .text_color(muted)
                .child("You are asked once more, with the list of what it may do."),
        );
    }
    if let Some(remove) = actions::remove(listing, ws, cx) {
        block = block.child(div().flex().pt(px(size::GAP_L)).child(remove));
    }
    block
}

/// The tier's mark, what it means, and the source by name.
fn origin(listing: &Listing, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let source = match listing.source.as_str() {
        "" => "none, found on this machine",
        source => source,
    };
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XS))
        .child(
            div()
                .group_name()
                .text_color(muted)
                .child("Where it comes from"),
        )
        .child(tier::mark(listing.tier, cx))
        .child(
            div()
                .small()
                .text_color(muted)
                .child(tier::about(listing.tier)),
        )
        .child(fact("Source", source.to_owned(), cx))
        .children(listing.upstream.as_ref().map(|upstream| {
            div()
                .flex()
                .flex_col()
                .gap(px(size::GAP_XS))
                .child(fact("From", upstream.clone(), cx))
                .child(div().small().text_color(muted).child(
                    "Fetched from the author's repository; not copied or changed by duskcode.",
                ))
        }))
}

/// The version, the dates in words, the license, and the addresses of its page and its
/// origin with the external glyph.
fn details(listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut block = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XS))
        .child(div().group_name().text_color(muted).child("Details"))
        .child(fact("Version", listing.version.clone(), cx));
    if let Some(date) = day(listing.created.as_deref()) {
        block = block.child(fact("Released", date, cx));
    }
    if let Some(date) = day(listing.updated.as_deref()) {
        block = block.child(fact("Updated", date, cx));
    }
    if let Some(version) = &listing.installed {
        block = block.child(fact("Installed", version.clone(), cx));
    }
    if !listing.license.is_empty() {
        block = block.child(fact("License", listing.license.clone(), cx));
    }
    let links = [
        ("detail-homepage", "Page", listing.homepage.as_deref()),
        ("detail-origin", "Origin", listing.origin.as_deref()),
    ];
    for (id, label, url) in links {
        if let Some(url) = url {
            block = block.children(link(id, label, url, cx));
        }
    }
    block
}

/// A label and an `https` address shown as its host and path, opened on a click; none
/// for an address that is not one.
fn link(id: &'static str, label: &'static str, url: &str, cx: &App) -> Option<Div> {
    let muted = cx.theme().muted_foreground;
    let shown = address(url)?;
    let url = url.to_owned();
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .small()
            .child(
                div()
                    .flex_none()
                    .w(px(SIDE / 3.))
                    .text_color(muted)
                    .child(label),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Button::new(id)
                        .link()
                        .icon(Icon::new(IconName::ExternalLink))
                        .label(shown)
                        .tooltip(url.clone())
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                ),
            ),
    )
}

/// A label and its value on one row.
fn fact(label: &'static str, value: String, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .small()
        .child(
            div()
                .flex_none()
                .w(px(SIDE / 3.))
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(capsule::clip(&value, LINE_CHARS)),
        )
}

/// A titled list of plain lines.
fn list(title: &str, lines: Vec<String>, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XS))
        .child(div().group_name().text_color(muted).child(title.to_owned()))
        .children(
            lines
                .into_iter()
                .map(|line| div().small().child(capsule::clip(&line, LINE_CHARS))),
        )
}

/// A third party's text with its line breaks kept and every other control a space, at
/// most `max` characters.
fn paragraphs(text: &str, max: usize) -> String {
    let mut kept: String = text
        .trim()
        .chars()
        .take(max)
        .map(|c| match c {
            '\n' => '\n',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    if text.trim().chars().nth(max).is_some() {
        kept.push('\u{2026}');
    }
    kept
}

/// An `https` link as its host and path, the way it is shown; none for anything else, or
/// for a link that names a user before its host and so could pass for another site.
fn address(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, tail) = rest.split_at(end);
    if host.is_empty() || host.contains('@') || host.contains('\\') {
        return None;
    }
    let path = tail.split(['?', '#']).next().unwrap_or("");
    Some(format!("{host}{}", path.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::{address, paragraphs};

    #[test]
    fn a_link_shows_its_real_host_or_nothing() {
        assert_eq!(
            address("https://github.com/duskcode/plugins/telegram/").as_deref(),
            Some("github.com/duskcode/plugins/telegram")
        );
        assert_eq!(
            address("https://example.com?q=1#top").as_deref(),
            Some("example.com")
        );
        assert_eq!(address("http://example.com"), None);
        assert_eq!(address("javascript:alert(1)"), None);
        assert_eq!(address("https://good.com@evil.com/x"), None);
        assert_eq!(address("https://evil.com\\@good.com"), None);
        assert_eq!(address("https://a.com/\nb"), None);
        assert_eq!(address("https:///path"), None);
    }

    #[test]
    fn paragraphs_keep_breaks_and_drop_controls() {
        assert_eq!(paragraphs("a\nb\u{1b}[2J", 50), "a\nb [2J");
        assert_eq!(paragraphs("ééé", 2), "éé\u{2026}");
    }
}
