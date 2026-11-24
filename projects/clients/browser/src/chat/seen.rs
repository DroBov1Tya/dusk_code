//! Unread chats: the core says which are; this page tells it the open chat was read
//! while the tab is visible.

use dusk_protocol::thread::Thread;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::{JsCast, closure::Closure};

use crate::{
    api,
    state::{AppState, View},
};

/// The chat to mark read now: the open one, when it is unread and the tab is visible.
fn due(threads: &[Thread], view: &View, visible: bool) -> Option<String> {
    let View::Chat(Some(open)) = view else {
        return None;
    };
    (visible
        && threads
            .iter()
            .any(|thread| thread.id == *open && thread.unread))
    .then(|| open.clone())
}

/// Marks the open chat read whenever it is unread while the tab is visible: as it opens,
/// as the tab comes back, and as something new arrives in it.
pub fn watch(app: AppState) {
    let visible = RwSignal::new(!hidden());
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        let changed = Closure::<dyn FnMut()>::new(move || visible.set(!hidden()));
        let added = document
            .add_event_listener_with_callback("visibilitychange", changed.as_ref().unchecked_ref());
        if added.is_ok() {
            // The page lives as long as the document; the listener does too.
            changed.forget();
        }
    }
    Effect::new(move |_| {
        let view = app.view.get();
        let Some(id) = app
            .threads
            .with(|threads| due(threads, &view, visible.get()))
        else {
            return;
        };
        heard(app, &id, false);
        spawn_local(async move {
            if let Err(error) = api::seen(&id).await {
                app.fail(error);
            }
        });
    });
}

/// The core said the chat turned unread or read.
pub fn heard(app: AppState, thread_id: &str, unread: bool) {
    app.threads.update(|threads| {
        if let Some(thread) = threads.iter_mut().find(|thread| thread.id == thread_id) {
            thread.unread = unread;
        }
    });
}

fn hidden() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.hidden())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, unread: bool) -> Thread {
        Thread {
            id: id.into(),
            source: "web".into(),
            created_at: 0,
            title: None,
            active_at: 0,
            parent_thread_id: None,
            agent: None,
            name: None,
            project: None,
            temporary: false,
            owner: None,
            unread,
            pulls: Vec::new(),
            branch: None,
        }
    }

    #[test]
    fn only_the_open_unread_chat_in_a_visible_tab_is_marked() {
        let threads = [thread("a", true), thread("b", false)];
        let open = |id: &str| View::Chat(Some(id.into()));
        assert_eq!(due(&threads, &open("a"), true).as_deref(), Some("a"));
        assert_eq!(due(&threads, &open("a"), false), None, "tab hidden");
        assert_eq!(due(&threads, &open("b"), true), None, "already read");
        assert_eq!(due(&threads, &View::Chat(None), true), None, "a draft");
        assert_eq!(due(&threads, &View::Artifacts, true), None);
    }
}
