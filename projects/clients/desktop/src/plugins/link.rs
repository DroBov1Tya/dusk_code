//! What opening, closing and using a part asks the core, and the views, pictures and
//! notes it sends back for the parts this window opened.

use dusk_protocol::plugin::{Opens, Place, UiEvent, Widget, check_view};
use gpui_kit::*;

use super::{Dock, Key};
use crate::{
    modal::Modal,
    pages::settings::SettingsPage,
    workspace::{Center, Workspace},
};

/// The longest plugin note the window shows; the rest is cut.
const MAX_NOTICE: usize = 300;

impl Workspace {
    /// Opens a part where its manifest says, and asks the core for its view; a part that
    /// is open already only comes forward.
    pub fn open_plugin_part(&mut self, key: Key, cx: &mut Context<Self>) {
        if self.show_plugin_part(key.clone(), cx) {
            let asked = key.clone();
            self.core.call(
                cx,
                move |client| async move { client.plugins().open(&asked.plugin, &asked.part).await },
                move |ws, result, cx| {
                    if let Err(error) = result {
                        ws.drop_plugin_part(&key);
                        ws.fail(error, cx);
                    }
                },
            );
        }
    }

    /// Puts a part in its place in the window, without asking the core. True when it was
    /// not open before.
    pub fn show_plugin_part(&mut self, key: Key, cx: &mut Context<Self>) -> bool {
        let Some(part) = super::part(self, &key).cloned() else {
            return false;
        };
        self.plugins.menu = false;
        let new = !self.plugins.is_open(&key, self.modal.as_ref());
        if part.place == Place::Settings {
            if let Some(shown) = self.plugins.settings.clone().filter(|shown| *shown != key) {
                self.close_plugin_part(shown, cx);
            }
            self.plugins.settings = Some(key);
            self.center = Center::Settings;
            self.settings_page = SettingsPage::Plugins;
        } else {
            match part.opens {
                Opens::RightPanel => bring(&mut self.plugins.right, key),
                Opens::BottomPanel => bring(&mut self.plugins.bottom, key),
                Opens::Window => {
                    if let Some(Modal::Plugin(shown)) = &self.modal
                        && *shown != key
                    {
                        let shown = shown.clone();
                        self.close_plugin_part(shown, cx);
                    }
                    self.modal = Some(Modal::Plugin(key));
                }
            }
        }
        cx.notify();
        new
    }

    /// Takes a part out of the window and tells the core it closed.
    pub fn close_plugin_part(&mut self, key: Key, cx: &mut Context<Self>) {
        self.drop_plugin_part(&key);
        if matches!(&self.modal, Some(Modal::Plugin(shown)) if *shown == key) {
            self.modal = None;
        }
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.plugins().close(&key.plugin, &key.part).await },
            |_, result, _| {
                // A plugin that stopped meanwhile has nothing to close: not news.
                if let Err(error) = result {
                    tracing::debug!("plugin close: {error:#}");
                }
            },
        );
    }

    /// Takes a part out of every place it shows in, and what was kept for it.
    fn drop_plugin_part(&mut self, key: &Key) {
        let plugins = &mut self.plugins;
        remove(&mut plugins.right, key);
        remove(&mut plugins.bottom, key);
        if plugins.settings.as_ref() == Some(key) {
            plugins.settings = None;
        }
        plugins.forget(key);
    }

    /// A widget of an open part was used.
    pub fn plugin_event(
        &mut self,
        key: &Key,
        widget: &str,
        event: UiEvent,
        cx: &mut Context<Self>,
    ) {
        let (key, widget) = (key.clone(), widget.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                let plugins = client.plugins();
                plugins.event(&key.plugin, &key.part, &widget, event).await
            },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }

    pub fn plugin_view(
        &mut self,
        plugin: String,
        part: String,
        root: Widget,
        cx: &mut Context<Self>,
    ) {
        let key = Key { plugin, part };
        if !self.plugins.is_open(&key, self.modal.as_ref()) {
            return;
        }
        // The core checked it; a view too large to draw is refused here as well.
        if let Err(reason) = check_view(&root) {
            self.fail(anyhow::anyhow!("{}: {reason}", key.plugin), cx);
            return;
        }
        self.plugins.views.insert(key, root);
        cx.notify();
    }

    pub fn plugin_frame(
        &mut self,
        plugin: String,
        part: String,
        widget: String,
        jpeg: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        let key = Key { plugin, part };
        if self.plugins.is_open(&key, self.modal.as_ref()) {
            self.take_frame((key, widget), jpeg, cx);
        }
    }

    /// A plugin's short note, shown for a moment over any page: news, not an error.
    pub fn plugin_notice(&mut self, plugin: &str, text: &str, cx: &mut Context<Self>) {
        let text: String = text.chars().take(MAX_NOTICE).collect();
        self.toast(format!("{plugin}: {text}"), cx);
    }

    /// Closes, without a word to the core, the parts of plugins that are gone from the
    /// status: they were switched off and their process has ended.
    pub fn prune_plugin_parts(&mut self, cx: &mut Context<Self>) {
        let listed = |key: &Key| super::part(self, key).is_some();
        let mut gone: Vec<Key> = self
            .plugins
            .right
            .tabs
            .iter()
            .chain(&self.plugins.bottom.tabs)
            .chain(&self.plugins.settings)
            .filter(|key| !listed(key))
            .cloned()
            .collect();
        if let Some(Modal::Plugin(key)) = &self.modal
            && !listed(key)
        {
            gone.push(key.clone());
            self.modal = None;
        }
        for key in &gone {
            self.drop_plugin_part(key);
        }
        if !gone.is_empty() {
            cx.notify();
        }
    }
}

/// Shows the part's tab, adding it when it is not there, and unfolds the dock.
fn bring(dock: &mut Dock, key: Key) {
    match dock.tabs.iter().position(|tab| *tab == key) {
        Some(index) => dock.active = index,
        None => {
            dock.tabs.push(key);
            dock.active = dock.tabs.len() - 1;
        }
    }
    dock.collapsed = false;
    dock.strip.scroll_to_item(dock.active);
}

fn remove(dock: &mut Dock, key: &Key) {
    let Some(index) = dock.tabs.iter().position(|tab| tab == key) else {
        return;
    };
    dock.tabs.remove(index);
    if dock.active > index || dock.active >= dock.tabs.len() {
        dock.active = dock.active.saturating_sub(1);
    }
}
