//! The Workshop: the plugins of the store, read through the core, each with a button that
//! installs, updates or removes it after a yes. The core checks the list, the package and
//! the manifest; this page only shows what it read and asks.

use std::collections::HashMap;

use dusk_protocol::plugin::Listing;
use gpui_kit::{
    component::{IconName, Sizable, button::Button},
    *,
};

use super::{Confirm, listing};
use crate::{modal::Modal, ui::widgets, workspace::Workspace};

#[derive(Default)]
pub struct Workshop {
    /// The Workshop is shown on the plugins shelf instead of the installed plugins.
    pub open: bool,
    /// The store's plugins, once read; kept while it is read again.
    pub listings: Option<Vec<Listing>>,
    pub loading: bool,
    /// Why the store could not be read.
    pub error: Option<String>,
    /// Plugins being installed or removed, with what the row says meanwhile.
    pub busy: HashMap<String, &'static str>,
    /// Why the last install or removal of a plugin failed.
    pub failed: HashMap<String, String>,
}

pub fn render(ws: &Workspace, query: &str, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let Some(listings) = &shop.listings else {
        return match &shop.error {
            Some(error) if !shop.loading => unreachable_store(error, cx),
            _ => crate::ui::loading("Reading the store...", cx),
        };
    };
    let shown: Vec<&Listing> = listings
        .iter()
        .filter(|listing| super::matches(query, &[&listing.name, &listing.description]))
        .collect();
    let mut page = div().flex().flex_col().gap_3();
    if let Some(error) = shop.error.as_ref().filter(|_| !shop.loading) {
        page = page.child(widgets::muted(
            format!("The store could not be read again: {error}"),
            cx,
        ));
    }
    if shown.is_empty() {
        let (title, hint) = if listings.is_empty() {
            ("The store is empty", "No plugin is offered yet.")
        } else {
            ("Nothing matches", "Try another word.")
        };
        return page.child(crate::ui::empty(IconName::Inbox, title, hint, cx));
    }
    let mut rows = widgets::card(cx).gap_0().p_0().max_w(px(880.));
    for (index, listing) in shown.into_iter().enumerate() {
        rows = rows.child(listing::render(ws, listing, index > 0, cx));
    }
    page.child(rows)
}

/// The store did not answer: what went wrong and a way to ask again.
fn unreachable_store(error: &str, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(crate::ui::empty(
            IconName::Inbox,
            "The store could not be read",
            error,
            cx,
        ))
        .child(
            Button::new("workshop-retry")
                .small()
                .outline()
                .label("Retry")
                .on_click(cx.listener(|ws, _, _, cx| ws.load_store(cx))),
        )
}

impl Workspace {
    pub fn load_store(&mut self, cx: &mut Context<Self>) {
        if self.workshop.loading {
            return;
        }
        self.workshop.loading = true;
        self.core.call(
            cx,
            |client| async move { client.plugins().store().await },
            |ws, result, cx| {
                let shop = &mut ws.workshop;
                shop.loading = false;
                match result {
                    Ok(listings) => {
                        shop.listings = Some(listings);
                        shop.error = None;
                    }
                    Err(error) => shop.error = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Asks before installing or removing; nothing happens until the yes.
    pub fn confirm_store(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.modal = Some(Modal::Workshop(confirm));
        cx.notify();
    }

    /// The yes of the confirmation window.
    pub fn store_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Workshop(confirm)) = self.modal.take() else {
            return;
        };
        cx.notify();
        match confirm {
            Confirm::Install(listing) => {
                let (name, version) = (listing.name, listing.version);
                self.store_work(
                    name.clone(),
                    "Installing...",
                    cx,
                    move |client| async move { client.plugins().install(&name, &version).await },
                );
            }
            Confirm::Remove(name) => {
                let removed = name.clone();
                self.store_work(name, "Removing...", cx, move |client| async move {
                    client.plugins().remove(&removed).await
                });
            }
        }
    }

    /// Runs one install or removal, shows it on the row, then reads the store again.
    fn store_work<F, Fut>(
        &mut self,
        name: String,
        doing: &'static str,
        cx: &mut Context<Self>,
        work: F,
    ) where
        F: FnOnce(std::sync::Arc<dusk_core::client::Client>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.workshop.failed.remove(&name);
        self.workshop.busy.insert(name.clone(), doing);
        self.core.call(cx, work, move |ws, result, cx| {
            ws.workshop.busy.remove(&name);
            if let Err(error) = result {
                ws.workshop.failed.insert(name, format!("{error:#}"));
            }
            ws.load_store(cx);
        });
    }
}
