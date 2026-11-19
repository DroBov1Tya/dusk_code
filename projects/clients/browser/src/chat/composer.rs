//! The message box: send, send now, stop, dictate, and the slash commands it offers.

use super::{data::ChatData, voice};
use crate::{
    api,
    state::{Chat, state},
    ui::{Icon, IconName},
};
use dusk_protocol::client::COMMANDS;
use leptos::{html, prelude::*, task::spawn_local};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq)]
enum Dictation {
    Idle,
    Listening,
    Transcribing,
}

/// What is typed and attached in a chat but not sent, kept while another page is shown.
#[derive(Default)]
pub struct Draft {
    pub text: String,
    pub pictures: Vec<super::pictures::Picture>,
}

#[component]
pub fn Composer(id: Option<String>, chat: Chat) -> impl IntoView {
    let app = state();
    let draft = app.take_draft(&id);
    let text = RwSignal::new(draft.text);
    let area = NodeRef::<html::Textarea>::new();
    let dictation = RwSignal::new(Dictation::Idle);
    let recording = StoredValue::new_local(None::<voice::Recording>);
    let recording_generation = StoredValue::new(0u64);
    let pictures = RwSignal::new(draft.pictures);
    // The box goes with its page; what it holds waits for this chat to be shown again.
    on_cleanup({
        let id = id.clone();
        move || {
            let draft = Draft {
                text: text.get_untracked(),
                pictures: pictures.get_untracked(),
            };
            app.keep_draft(id, draft);
        }
    });
    let alive = Arc::new(AtomicBool::new(true));
    on_cleanup({
        let alive = alive.clone();
        move || {
            alive.store(false, Ordering::Relaxed);
            if let Some(started) = recording.try_update_value(|r| r.take()).flatten() {
                started.finish();
            }
        }
    });
    let picker = NodeRef::<html::Input>::new();

    Effect::new({
        let id = id.clone();
        move |_| {
            if app.dictations.with(|pending| pending.contains_key(&id)) {
                let heard = app
                    .dictations
                    .try_update(|pending| pending.remove(&id))
                    .flatten()
                    .unwrap_or_default();
                text.update(|text| {
                    for part in heard {
                        if !text.is_empty() && !text.ends_with(' ') {
                            text.push(' ');
                        }
                        text.push_str(part.trim());
                    }
                });
            }
        }
    });

    Effect::new({
        let id = id.clone();
        move |_| {
            if app
                .pending_pictures
                .with(|pending| pending.contains_key(&id))
            {
                let added = app
                    .pending_pictures
                    .try_update(|pending| pending.remove(&id))
                    .flatten()
                    .unwrap_or_default();
                pictures.update(|pictures| pictures.extend(added));
            }
        }
    });
    // After /undo the core hands the message back to edit.
    Effect::new(move |_| {
        if let Some(offered) = chat.with(|c| c.offered.clone()) {
            text.update(|draft| restore_offered(draft, offered));
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
    let dictation_id = id.clone();
    let toggle_dictation = move |_| {
        let alive = alive.clone();
        let dictation_id = dictation_id.clone();
        let generation = app.draft_generation.get_untracked();
        match dictation.get_untracked() {
            Dictation::Idle => {
                recording_generation.set_value(generation);
                dictation.set(Dictation::Transcribing);
                spawn_local(async move {
                    match voice::start_recording().await {
                        Ok(started) if !alive.load(Ordering::Relaxed) => {
                            started.finish();
                        }
                        Ok(started) => {
                            recording.set_value(Some(started));
                            dictation.set(Dictation::Listening);
                        }
                        Err(error) => {
                            if alive.load(Ordering::Relaxed) {
                                dictation.set(Dictation::Idle);
                            }
                            app.fail(error);
                        }
                    }
                });
            }
            Dictation::Listening => {
                let generation = recording_generation.get_value();
                let Some(started) = recording.try_update_value(|r| r.take()).flatten() else {
                    return;
                };
                dictation.set(Dictation::Transcribing);
                let audio = started.finish();
                spawn_local(async move {
                    match api::transcribe(audio).await {
                        Ok(heard) => {
                            if dictation_id.as_deref().map_or_else(
                                || app.draft_generation.get_untracked() == generation,
                                |id| app.chat(id).is_some(),
                            ) {
                                app.dictations.update(|pending| {
                                    pending.entry(dictation_id).or_default().push(heard)
                                });
                            }
                        }
                        Err(error) => app.fail(error),
                    }
                    if alive.load(Ordering::Relaxed) {
                        dictation.set(Dictation::Idle);
                    }
                });
            }
            Dictation::Transcribing => {}
        }
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
    let panel_open = id.is_some();
    let access_id = id.clone();
    let paste_id = id.clone();
    let drop_id = id.clone();
    let pick_id = id.clone();
    view! {
        <div class="composer-wrap">
            {panel_open.then(|| view! { <super::panel::Panel /> })}
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
                            super::pictures::add_files(files, paste_id.clone());
                        }
                    }
                    on:dragover=move |ev: leptos::ev::DragEvent| ev.prevent_default()
                    on:drop=move |ev: leptos::ev::DragEvent| {
                        ev.prevent_default();
                        super::pictures::add_files(super::pictures::dropped(&ev), drop_id.clone());
                    }
                ></textarea>
                <div class="composer-actions">
                    <input type="file" accept="image/png,image/jpeg" multiple hidden node_ref=picker
                        on:change=move |ev| {
                            if let Some(input) = super::pictures::input_of(&ev) {
                                super::pictures::pick(&input, pick_id.clone());
                            }
                        } />
                    <button class="icon round" title="Attach pictures" aria-label="Attach pictures" on:click=move |_| {
                        if let Some(input) = picker.get() {
                            input.click();
                        }
                    }><Icon name=IconName::Plus /></button>
                    <super::controls::Access id=access_id chat />
                    <div class="composer-submit">
                        <Show when=working>
                            <button class="ghost" on:click=move |_| stop.run(()) title="Stop the turn">
                                <Icon name=IconName::Square />"Stop"
                            </button>
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
                            <Icon name=IconName::Mic />
                        </button>
                        <button class="primary round" on:click=move |_| send.run(false)
                            title=move || if working() { "Queue it" } else { "Send" }>
                            <Icon name=IconName::ArrowUp />
                        </button>
                    </div>
                </div>
            </div>
            <div class="composer-under">
                <super::controls::Mode />
                <Show when=move || voice::speaking()><span class="muted">"speaking\u{2026}"</span></Show>
                <super::controls::Model />
            </div>
            <super::controls::Context chat />
        </div>
    }
}

/// Restore the undone message before any text already in the draft.
fn restore_offered(draft: &mut String, mut offered: String) {
    if !offered.is_empty() && !draft.is_empty() {
        offered.push_str("\n\n");
    }
    offered.push_str(draft);
    *draft = offered;
}

#[cfg(test)]
mod tests {
    use super::restore_offered;

    #[test]
    fn undone_text_keeps_an_existing_draft() {
        let mut draft = "new text".to_owned();
        restore_offered(&mut draft, "undone text".to_owned());
        assert_eq!(draft, "undone text\n\nnew text");
        let mut empty = String::new();
        restore_offered(&mut empty, "undone text".to_owned());
        assert_eq!(empty, "undone text");
    }
}
