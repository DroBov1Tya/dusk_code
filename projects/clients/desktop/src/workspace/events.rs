//! What the core sends: events and status, folded into the view as they come.

use dusk_protocol::client::{Event, Status};
use gpui_kit::*;
use tokio::sync::{mpsc, watch};

use super::Workspace;

impl Workspace {
    pub fn pump_events(
        &mut self,
        mut events: mpsc::UnboundedReceiver<Event>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                let alive = this.update(cx, |this, cx| {
                    this.on_event(event, cx);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    pub fn pump_status(&mut self, mut status: watch::Receiver<Status>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while status.changed().await.is_ok() {
                let next = status.borrow_and_update().clone();
                let alive = this.update(cx, |this, cx| {
                    // A sign-in that just ended brought a provider: show it.
                    let signed = this.status.chatgpt != next.chatgpt;
                    this.status = next;
                    this.prune_plugin_parts(cx);
                    if signed {
                        this.load_settings(cx);
                    }
                    this.fetch_update(cx);
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Thread { thread_id, event } => {
                if let dusk_protocol::thread::ThreadEvent::OutputRate { tokens_per_sec, .. } =
                    &*event
                {
                    self.rates.insert(thread_id.clone(), *tokens_per_sec);
                }
                let titled = matches!(
                    *event,
                    dusk_protocol::thread::ThreadEvent::TitleChanged { .. }
                );
                self.notify_event(&thread_id, &event, cx);
                self.watch_plan(&thread_id, &event, cx);
                if matches!(*event, dusk_protocol::thread::ThreadEvent::Tokens { .. }) {
                    self.context_counted(&thread_id, cx);
                }
                if matches!(
                    *event,
                    dusk_protocol::thread::ThreadEvent::ToolFinished { .. }
                ) && self.shown.as_deref() == Some(thread_id.as_str())
                {
                    self.load_touched(cx);
                }
                // A job starts inside a tool and ends as a message of its own; the Jobs
                // module is hidden while it has none, so nothing else would ask for them.
                if matches!(
                    *event,
                    dusk_protocol::thread::ThreadEvent::ToolFinished { .. }
                        | dusk_protocol::thread::ThreadEvent::FromJob(_)
                ) && self.shown.as_deref() == Some(thread_id.as_str())
                {
                    self.load_jobs(cx);
                    self.load_sources(cx);
                }
                let mut finished = false;
                if let Some(chat) = self.chats.get_mut(&thread_id) {
                    let was_working = chat.working;
                    chat.apply(*event);
                    finished = was_working && !chat.working;
                }
                if finished {
                    self.rates.remove(&thread_id);
                    self.send_next_pending(&thread_id, cx);
                    // A source that went with the turn now reads as sent.
                    if self.shown.as_deref() == Some(thread_id.as_str()) {
                        self.dawn(cx);
                        self.load_sources(cx);
                    }
                }
                if titled {
                    self.reload_threads(cx);
                }
            }
            Event::Terminal { id, data } => self.terminal_output(&id, &data, cx),
            Event::TerminalClosed { id, code } => self.terminal_closed(&id, code, cx),
            Event::PluginView { plugin, part, root } => self.plugin_view(plugin, part, root, cx),
            Event::PluginFrame {
                plugin,
                part,
                widget,
                jpeg,
            } => self.plugin_frame(plugin, part, widget, jpeg, cx),
            Event::PluginNotice { plugin, text } => self.plugin_notice(&plugin, &text, cx),
            Event::Ended { thread_id } => {
                if let Some(chat) = self.chats.get_mut(&thread_id) {
                    chat.working = false;
                }
            }
        }
    }

    pub fn reload_threads(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.threads().list().await },
            |this, result, cx| match result {
                Ok(mut threads) => {
                    threads.retain(|thread| thread.parent_thread_id.is_none());
                    threads.sort_by_key(|thread| std::cmp::Reverse(thread.active_at));
                    this.threads = threads;
                    cx.notify();
                }
                Err(error) => this.fail(error, cx),
            },
        );
    }
}
