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
    let pictures = RwSignal::new(Vec::<super::pictures::Picture>::new());
    let picker = NodeRef::<html::Input>::new();

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
            let attached = pictures.get_untracked();
            if value.trim().is_empty() && attached.is_empty() {
                return;
            }
            text.set(String::new());
            pictures.set(Vec::new());
            app.send(id.clone(), value, now, attached);
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
    let access_id = id.clone();
    view! {
        // The box between two sides of one width, the context ring on the right; the mode
        // and the model under it.
        <div class="composer-wrap">
            <div class="composer-side"></div>
            <div class="composer-column">
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
                    <super::pictures::Strip pictures />
                    <textarea
                        node_ref=area
                        rows="1"
                        placeholder="Ask anything. Enter sends, Shift+Enter starts a new line."
                        prop:value=move || text.get()
                        on:input=move |ev| text.set(event_target_value(&ev))
                        on:keydown=on_key
                        on:paste=move |ev: leptos::ev::ClipboardEvent| {
                            let files = super::pictures::pasted(&ev);
                            if !files.is_empty() {
                                ev.prevent_default();
                                super::pictures::add_files(files, pictures);
                            }
                        }
                        on:dragover=move |ev: leptos::ev::DragEvent| ev.prevent_default()
                        on:drop=move |ev: leptos::ev::DragEvent| {
                            ev.prevent_default();
                            super::pictures::add_files(super::pictures::dropped(&ev), pictures);
                        }
                    ></textarea>
                    <div class="composer-actions">
                        <input type="file" accept="image/png,image/jpeg" multiple hidden node_ref=picker
                            on:change=move |ev| {
                                if let Some(input) = super::pictures::input_of(&ev) {
                                    super::pictures::pick(&input, pictures);
                                }
                            } />
                        <button class="icon round" title="Attach pictures" on:click=move |_| {
                            if let Some(input) = picker.get() {
                                input.click();
                            }
                        }>"+"</button>
                        <super::controls::Access id=access_id chat />
                        <span class="grow"></span>
                        <Show when=working>
                            <button class="ghost" on:click=move |_| stop.run(()) title="Stop the turn">"Stop"</button>
                            <button class="ghost" on:click=move |_| send.run(true)
                                title="Interrupt and send this now">"Send now"</button>
                        </Show>
                        <button class=move || if dictation.get() == Dictation::Listening { "icon round live" } else { "icon round" }
                            on:click=toggle_dictation
                            title=move || match dictation.get() {
                                Dictation::Idle => "Dictate",
                                Dictation::Listening => "Stop dictating",
                                Dictation::Transcribing => "Writing it down",
                            }>
                            <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true">
                                <rect x="9" y="3" width="6" height="11" rx="3" />
                                <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
                            </svg>
                        </button>
                        <button class="primary round" on:click=move |_| send.run(false)
                            title=move || if working() { "Queue it" } else { "Send" }>
                            <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true">
                                <path d="M12 19V5M6 11l6-6 6 6" />
                            </svg>
                        </button>
                    </div>
                </div>
                <div class="composer-under">
                    <super::controls::Mode />
                    <Show when=move || voice::speaking()><span class="muted">"speaking\u{2026}"</span></Show>
                    <super::controls::Model />
                </div>
            </div>
            <div class="composer-side"><super::controls::Context chat /></div>
        </div>
    }
}
