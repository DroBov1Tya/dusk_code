//! A card of Workshop listings the agent put in a chat: its heading, then a row per
//! listing with its glyph, name, kind and a line of what it is, and the
//! Workshop's own slot at the right: Add (the Workshop's install, with its
//! confirmation), Update, a turning ring or "Installed". Whether a row is installed is
//! read from the store now, not from the card, which only says what was suggested. A
//! click on a row opens the listing's window over the Workshop of its own shelf.

use dusk_protocol::plugin::{Listing, Order, Scope, Suggested, Suggestion, Tier};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Workshop, actions, clip, kind_word, placeholder, tier};
use crate::{
    pages::chat::transcript::Item,
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The glyph's square at the left of a row.
const TILE: f32 = 40.;
const LABEL_CHARS: usize = 80;
const LINE_CHARS: usize = 200;

/// Every listing of every kind, as the store last gave them for the chat's cards.
#[derive(Default)]
pub struct Read {
    pub listings: Option<Vec<Listing>>,
    pub loading: bool,
    /// Why the store could not be read; the rows wait for Retry.
    pub error: Option<String>,
}

impl Read {
    /// The row's listing as the store gives it now, by its source and name.
    pub fn live(&self, row: &Suggested) -> Option<&Listing> {
        self.listings
            .as_deref()
            .and_then(|listings| row.find(listings))
    }
}

/// What a row's slot holds: the Workshop's own, with Add for the install of a listing
/// not here; a quiet word while the store is read or when it no longer lists it.
#[derive(Debug, PartialEq, Eq)]
pub enum Slot {
    Add,
    /// What the Workshop's card shows: Update, Retry, a ring, or "Installed".
    Workshop,
    Reading,
    Gone,
}

pub fn slot(shop: &Workshop, live: Option<&Listing>) -> Slot {
    match live {
        None if shop.suggested.loading || shop.suggested.listings.is_none() => Slot::Reading,
        None => Slot::Gone,
        Some(listing)
            if listing.installed.is_none()
                && listing.upstream.is_none()
                && !shop.busy.contains_key(&listing.name)
                && !shop.failed.contains_key(&listing.name) =>
        {
            Slot::Add
        }
        Some(_) => Slot::Workshop,
    }
}

/// The card at item `key` of a chat.
pub fn render(
    ws: &Workspace,
    key: &str,
    card: &Suggestion,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let mut body = widgets::card(cx).gap_0().p_0().w_full().child(
        div()
            .px(px(size::GAP_L))
            .py(px(size::GAP_M))
            .body()
            .font_weight(FontWeight::SEMIBOLD)
            .child(clip(&card.label, LABEL_CHARS)),
    );
    if let Some(error) = ws.workshop.suggested.error.as_ref() {
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(size::GAP_S))
                .px(px(size::GAP_L))
                .pb(px(size::GAP_S))
                .meta()
                .text_color(muted)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(format!("The Workshop could not be read: {error}")),
                )
                .child(
                    Button::new(SharedString::from(format!("suggested-retry-{key}")))
                        .ghost()
                        .label("Retry")
                        .on_click(cx.listener(|ws, _, _, cx| ws.load_suggested(cx))),
                ),
        );
    }
    for (index, row) in card.listings.iter().enumerate() {
        body = body.child(render_row(ws, &format!("{key}-{index}"), row, cx));
    }
    body.into_any_element()
}

fn render_row(
    ws: &Workspace,
    key: &str,
    row: &Suggested,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, border, hover) = (theme.muted_foreground, theme.border, theme.list_hover);
    let live = ws.workshop.suggested.live(row);
    // A cover is drawn for a card's width; cut to a square it shows a stray word, so the
    // row has the kind's glyph on the name's gradient instead.
    let tile = placeholder::tile(&row.name, row.kind, TILE, cx);
    let about = match row.skills {
        // Nobody vouches for it, so where it comes from goes before what it says it is.
        _ if row.tier == Tier::Unverified => format!(
            "from {} \u{b7} {}",
            clip(&row.source, 32),
            clip(&row.description, LINE_CHARS)
        ),
        0 => clip(&row.description, LINE_CHARS),
        count => format!(
            "{} \u{b7} {}",
            widgets::count(count, "skill"),
            clip(&row.description, LINE_CHARS)
        ),
    };
    let text = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(size::GAP_S))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .child(clip(&row.name, LABEL_CHARS)),
                )
                .child(
                    div()
                        .flex_none()
                        .meta()
                        .text_color(muted)
                        .child(kind_word(row.kind)),
                )
                .child(tier::mark(row.tier, cx)),
        )
        .child(div().meta().text_color(muted).truncate().child(about));
    // The slot keeps its click, so it does not open the window too.
    let slot = div()
        .flex_none()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(slot_element(ws, live, cx));
    let opened = live.cloned();
    div()
        .id(SharedString::from(format!("suggested-{key}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_M))
        .px(px(size::GAP_L))
        .py(px(size::GAP_S))
        .border_t_1()
        .border_color(border)
        .when(opened.is_some(), |row| {
            row.cursor_pointer().hover(move |row| row.bg(hover))
        })
        .on_click(cx.listener(move |ws, _, window, cx| {
            if let Some(listing) = &opened {
                ws.open_suggested(listing, window, cx);
            }
        }))
        .child(tile)
        .child(text)
        .child(slot)
}

fn slot_element(ws: &Workspace, live: Option<&Listing>, cx: &mut Context<Workspace>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    match (slot(&ws.workshop, live), live) {
        (Slot::Add, Some(listing)) => actions::install(listing, "Add".into(), false, cx)
            .h(px(28.))
            .into_any_element(),
        (Slot::Workshop, Some(listing)) => actions::slot(listing, ws, true, cx),
        (Slot::Reading, _) => div()
            .meta()
            .text_color(muted)
            .child("Reading...")
            .into_any_element(),
        _ => div()
            .meta()
            .text_color(muted)
            .child("Not in the Workshop now")
            .into_any_element(),
    }
}

impl Workspace {
    /// Opens a row's listing over the Workshop of its own shelf, as a click on its capsule
    /// there would.
    pub fn open_suggested(
        &mut self,
        listing: &Listing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scope = Scope::ALL
            .into_iter()
            .filter(|scope| *scope != Scope::Connectors)
            .find(|scope| scope.holds(listing))
            .unwrap_or_default();
        self.center = crate::workspace::Center::Customize;
        self.discover(scope, cx);
        self.open_listing(listing, window, cx);
    }

    /// Reads every listing for the cards of the chat in view, once, when it has one.
    pub fn ensure_suggested(&mut self, cx: &mut Context<Self>) {
        let read = &self.workshop.suggested;
        if read.listings.is_some() || read.loading || read.error.is_some() {
            return;
        }
        let carded = self
            .shown
            .as_ref()
            .and_then(|shown| self.chats.get(shown))
            .is_some_and(|chat| {
                chat.items
                    .iter()
                    .any(|item| matches!(item, Item::Suggestion(_)))
            });
        if carded {
            self.load_suggested(cx);
        }
    }

    /// Reads them again after an install or removal, when a card wanted them.
    pub fn reload_suggested(&mut self, cx: &mut Context<Self>) {
        if self.workshop.suggested.listings.is_some() {
            self.load_suggested(cx);
        }
    }

    pub fn load_suggested(&mut self, cx: &mut Context<Self>) {
        if self.workshop.suggested.loading {
            return;
        }
        self.workshop.suggested.loading = true;
        self.core.call(
            cx,
            |client| async move { client.plugins().store(Order::Trust, None, Vec::new()).await },
            |ws, result, cx| {
                let read = &mut ws.workshop.suggested;
                read.loading = false;
                match result {
                    Ok(list) => {
                        read.listings = Some(list.listings);
                        read.error = None;
                        ws.load_covers(cx);
                    }
                    Err(error) => read.error = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::plugin::{Kind, Listing, Suggested};

    use super::{Slot, Workshop, slot};

    fn listing(source: &str, name: &str) -> Listing {
        Listing {
            source: source.into(),
            name: name.into(),
            version: "1.0.0".into(),
            kind: Kind::Pack,
            permissions: vec!["reads the web".into()],
            ..Listing::default()
        }
    }

    /// A row's Add is the Workshop's install of the listing the store gives now, the one
    /// from the row's own source, and only while nothing else is shown in its place.
    #[test]
    fn a_row_offers_add_only_for_the_live_listing_not_here() {
        let mut shop = Workshop::default();
        let row = Suggested::of(&listing("official", "motion"));
        assert_eq!(slot(&shop, shop.suggested.live(&row)), Slot::Reading);

        let mut mirror = listing("friends", "motion");
        mirror.permissions = vec!["runs a program".into()];
        shop.suggested.listings = Some(vec![mirror, listing("official", "motion")]);
        let live = shop.suggested.live(&row).unwrap();
        assert_eq!(live.source, "official");
        assert_eq!(live.permissions, ["reads the web"]);
        assert_eq!(slot(&shop, Some(live)), Slot::Add);

        // At work, failed, installed or with an update: the Workshop's own slot.
        shop.busy.insert("motion".into(), "Installing...");
        assert_eq!(slot(&shop, shop.suggested.live(&row)), Slot::Workshop);
        shop.busy.clear();
        shop.failed.insert("motion".into(), "boom".into());
        assert_eq!(slot(&shop, shop.suggested.live(&row)), Slot::Workshop);
        shop.failed.clear();
        if let Some(listings) = &mut shop.suggested.listings {
            listings[1].installed = Some("1.0.0".into());
        }
        assert_eq!(slot(&shop, shop.suggested.live(&row)), Slot::Workshop);

        // Gone from the store: no button at all.
        let gone = Suggested::of(&listing("official", "gone"));
        assert_eq!(slot(&shop, shop.suggested.live(&gone)), Slot::Gone);
    }
}
