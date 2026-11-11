//! The message box: send, send now, stop, dictate, and the slash commands it offers.

use super::data::ChatData;
use crate::{
    api,
    state::{Chat, state},
    voice,
};
use dusk_protocol::client::COMMANDS;
use leptos::{html, prelude::*, task::spawn_local};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq)]
enum Dictation {
    Idle,
    Listening,
    Transcribing,
}

#[component]
pub fn Composer(id: Option<String>, chat: Chat) -> impl IntoView {
    let app = state();
    let text = RwSignal::new(String::new());
    let area = NodeRef::<html::Textarea>::new();
    let dictation = RwSignal::new(Dictation::Idle);
    let recording = StoredValue::new_local(None::<voice::Recording>);

    // After /undo the core hands the message back to edit.
    Effect::new(move |_| {
        if let Some(offered) = chat.with(|c| c.offered.clone()) {
            text.set(offered);
            chat.update(|c: &mut ChatData| c.offered = None);
        }
    });
    Effect::new(move |_| {
        text.track();
        if let Some(area) = area.get() {
            // Grows with its text up to a cap.
            let element: &web_sys::HtmlElement = area.unchecked_ref();
            let style = element.style();
            let _ = style.set_property("height", "auto");
            let height = area.scroll_height().min(220);
            let _ = style.set_property("height", &format!("{height}px"));
        }
    });

    let working = move || chat.with(|c| c.working);
    let send = Callback::new({
        let id = id.clone();
        move |now: bool| {
            let value = text.get_untracked();
            if value.trim().is_empty() {
                return;
            }
            text.set(String::new());
            app.send(id.clone(), value, now);
        }
    });
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" && !ev.shift_key() && !ev.is_composing() {
            ev.prevent_default();
            send.run(false);
        }
    };
    let stop = Callback::new({
        let id = id.clone();
        move |()| {
            if let Some(id) = id.clone() {
                crate::ui::run(async move { api::cancel(&id).await }, |()| ());
            }
        }
    });
    let toggle_dictation = move |_| match dictation.get_untracked() {
        Dictation::Idle => spawn_local(async move {
            match voice::start_recording().await {
                Ok(started) => {
                    recording.set_value(Some(started));
                    dictation.set(Dictation::Listening);
                }
                Err(error) => app.fail(error),
            }
        }),
        Dictation::Listening => {
            let Some(started) = recording.try_update_value(|r| r.take()).flatten() else {
                return;
            };
            dictation.set(Dictation::Transcribing);
            let audio = started.finish();
            spawn_local(async move {
                match api::transcribe(audio).await {
                    Ok(heard) => text.update(|t| {
                        if !t.is_empty() && !t.ends_with(' ') {
                            t.push(' ');
                        }
                        t.push_str(heard.trim());
                    }),
                    Err(error) => app.fail(error),
                }
                dictation.set(Dictation::Idle);
            });
        }
        Dictation::Transcribing => {}
    };

    let suggestions = move || {
        let typed = text.get();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(typed.as_str()))
            .map(|(name, help)| (*name, *help))
            .collect::<Vec<_>>()
    };
    view! {
        <div class="composer">
            <Show when=move || !suggestions().is_empty()>
                <ul class="suggest">
                    <For each=suggestions key=|(name, _)| *name let:entry>
                        <li on:click=move |_| text.set(format!("{} ", entry.0))>
                            <b>{entry.0}</b>" "<span class="muted">{entry.1}</span>
                        </li>
                    </For>
                </ul>
            </Show>
            <textarea
                node_ref=area
                rows="1"
                placeholder="Message, or / for commands"
                prop:value=move || text.get()
                on:input=move |ev| text.set(event_target_value(&ev))
                on:keydown=on_key
            ></textarea>
            <div class="composer-actions">
                <button class="ghost" on:click=toggle_dictation
                    title="Dictate with the speech model">
                    {move || match dictation.get() {
                        Dictation::Idle => "Listen",
                        Dictation::Listening => "Stop",
                        Dictation::Transcribing => "…",
                    }}
                </button>
                <span class="grow"></span>
                <Show when=working>
                    <button class="ghost" on:click=move |_| stop.run(()) title="Stop the turn">"Stop"</button>
                    <button class="ghost" on:click=move |_| send.run(true)
                        title="Interrupt and send this now">"Send now"</button>
                </Show>
                <button class="primary" on:click=move |_| send.run(false)>
                    {move || if working() { "Queue" } else { "Send" }}
                </button>
            </div>
        </div>
    }
}
