//! Unread chats: the core says which are; this window tells it a chat was read while the
//! chat is shown and the window is in front.

use dusk_protocol::thread::Thread;
use gpui_kit::*;

use super::{Center, Workspace};

/// The chat to mark read now: the one shown, when it is unread and the window is in front.
fn due<'a>(
    threads: &[Thread],
    shown: Option<&'a str>,
    chat: bool,
    active: bool,
) -> Option<&'a str> {
    let shown = shown.filter(|_| chat && active)?;
    threads
        .iter()
        .any(|thread| thread.id == shown && thread.unread)
        .then_some(shown)
}

/// Before each frame: the shown chat is read. It reads as read here at once; the core is
/// told, and tells the other clients.
pub fn settle(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let chat = ws.center == Center::Chat;
    let Some(id) = due(
        &ws.threads,
        ws.shown.as_deref(),
        chat,
        window.is_window_active(),
    ) else {
        return;
    };
    let id = id.to_owned();
    heard(ws, &id, false);
    ws.core.call(
        cx,
        move |client| async move { client.threads().seen(&id).await },
        |ws, result, cx| {
            if let Err(error) = result {
                ws.fail(error, cx);
            }
        },
    );
}

/// The core said the chat turned unread or read.
pub fn heard(ws: &mut Workspace, thread_id: &str, unread: bool) {
    if let Some(thread) = ws.threads.iter_mut().find(|thread| thread.id == thread_id) {
        thread.unread = unread;
    }
}

#[cfg(test)]
mod tests {
    use super::{Thread, due};

    fn thread(id: &str, unread: bool) -> Thread {
        Thread {
            id: id.into(),
            source: "gui".into(),
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
    fn only_the_shown_unread_chat_in_a_window_in_front_is_marked() {
        let threads = [thread("a", true), thread("b", false)];
        assert_eq!(due(&threads, Some("a"), true, true), Some("a"));
        assert_eq!(due(&threads, Some("a"), true, false), None, "window behind");
        assert_eq!(due(&threads, Some("a"), false, true), None, "another page");
        assert_eq!(due(&threads, Some("b"), true, true), None, "already read");
        assert_eq!(due(&threads, None, true, true), None);
    }
}
