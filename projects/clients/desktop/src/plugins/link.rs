//! What opening, closing and using a part asks the core, and the views, pictures and
//! notes it sends back for the parts this window opened.

use dusk_core::app::PhoneDependency;
use dusk_protocol::plugin::{MAX_TEXT, Opens, Place, UiEvent, Widget, check_view};
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

    pub(super) fn install_phone_dependency(&mut self, key: Key, cx: &mut Context<Self>) {
        let Some(dependency) = PhoneDependency::for_panel(&key.plugin, &key.part) else {
            return;
        };
        if !self.plugins.is_open(&key, self.modal.as_ref())
            || !self.plugins.dependency_installs.insert(key.clone())
        {
            return;
        }
        self.plugins.dependency_errors.remove(&key);
        cx.notify();
        self.core.call(
            cx,
            move |_| dependency.install(),
            move |ws, result, cx| {
                if !ws.plugins.is_open(&key, ws.modal.as_ref()) {
                    ws.finish_dependency_install(key, result, cx);
                    return;
                }
                let reopen = key.clone();
                ws.core.call(
                    cx,
                    move |client| async move {
                        client
                            .plugins()
                            .redetect(&reopen.plugin, &reopen.part)
                            .await
                    },
                    move |ws, redetected, cx| {
                        let result = match (result, redetected) {
                            (Err(install), Err(refresh)) => Err(anyhow::anyhow!(
                                "{install:#}\nCould not refresh the phone panel: {refresh:#}"
                            )),
                            (install, refresh) => install.and(refresh),
                        };
                        ws.finish_dependency_install(key, result, cx);
                    },
                );
            },
        );
    }

    fn finish_dependency_install(
        &mut self,
        key: Key,
        result: anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) {
        self.plugins.dependency_installs.remove(&key);
        if let Err(error) = result {
            if self.plugins.is_open(&key, self.modal.as_ref()) {
                self.plugins
                    .dependency_errors
                    .insert(key, format!("{error:#}"));
            } else {
                self.fail(error, cx);
            }
        }
        cx.notify();
    }

    /// A widget of an open part was used.
    pub fn plugin_event(
        &mut self,
        key: &Key,
        widget: &str,
        event: UiEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            event,
            UiEvent::Touch { .. }
                | UiEvent::Key { .. }
                | UiEvent::Text { .. }
                | UiEvent::Scroll { .. }
        ) {
            self.queue_screen_event(key, widget, event, cx);
            return;
        }
        let refreshes_phone = widget == "refresh"
            && event == UiEvent::Click
            && PhoneDependency::for_panel(&key.plugin, &key.part).is_some();
        if refreshes_phone {
            self.plugins.dependency_errors.remove(key);
            cx.notify();
        }
        let (key, widget) = (key.clone(), widget.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                let plugins = client.plugins();
                if refreshes_phone {
                    return plugins.redetect(&key.plugin, &key.part).await;
                }
                plugins.event(&key.plugin, &key.part, &widget, event).await
            },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }

    fn queue_screen_event(
        &mut self,
        key: &Key,
        widget: &str,
        event: UiEvent,
        cx: &mut Context<Self>,
    ) {
        let queue = &mut self.plugins.screen_queue;
        if let (Some((last_key, last_widget, UiEvent::Text { text: last })), UiEvent::Text { text }) =
            (queue.back_mut(), &event)
            && last_key == key
            && last_widget == widget
            && last.chars().count() + text.chars().count() <= MAX_TEXT
        {
            last.push_str(text);
        } else {
            queue.push_back((key.clone(), widget.to_owned(), event));
        }
        self.send_screen_event(cx);
    }

    fn send_screen_event(&mut self, cx: &mut Context<Self>) {
        if self.plugins.screen_sending {
            return;
        }
        let Some((key, widget, event)) = self.plugins.screen_queue.pop_front() else {
            return;
        };
        self.plugins.screen_sending = true;
        self.core.call(
            cx,
            move |client| async move {
                client
                    .plugins()
                    .event(&key.plugin, &key.part, &widget, event)
                    .await
            },
            |ws, result, cx| {
                ws.plugins.screen_sending = false;
                if let Err(error) = result {
                    ws.plugins.screen_queue.clear();
                    ws.fail(error, cx);
                }
                ws.send_screen_event(cx);
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
