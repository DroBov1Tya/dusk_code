//! The chat's sources, the files and long pastes attached to it, and a window to read
//! one: markdown rendered, other text as it is, a picture fitted, and anything else by its
//! size. Text comes in parts by the source's id, never by a path; a long one grows by a
//! part a press, up to `MAX_SHOWN`.

use crate::{
    api,
    chat::{blocks::copy, markdown::Markdown},
    ui::{Icon, IconName, run, size},
};
use dusk_protocol::thread::{Source, SourceChunk, SourceKind};
use leptos::prelude::*;

/// The most text the window holds; past it the rest stays in the file.
const MAX_SHOWN: usize = 1024 * 1024;

#[component]
pub fn SourcesTab(id: String) -> impl IntoView {
    let sources = RwSignal::new(Vec::<Source>::new());
    let open = RwSignal::new(None::<Source>);
    let thread = id.clone();
    run(async move { api::sources(&thread).await }, move |list| {
        sources.set(list)
    });
    view! {
        <div class="sources">
            <Show when=move || !sources.get().is_empty()
                fallback=|| view! { <p class="muted">"Nothing is attached to this chat."</p> }>
                {move || sources.get().into_iter().map(|source| {
                    let state = if source.pinned {
                        "sent with every request"
                    } else if source.delivered {
                        "sent once"
                    } else {
                        "goes with the next request"
                    };
                    let about = format!("{} \u{00B7} {state}", size(source.bytes));
                    let (name, opened) = (source.name.clone(), source.clone());
                    view! {
                        <button class="source-row" title="Read it"
                            on:click=move |_| open.set(Some(opened.clone()))>
                            <span class="source-name">{name}</span>
                            <span class="muted">{about}</span>
                        </button>
                    }
                }).collect_view()}
            </Show>
            {move || open.get().map(|source| view! { <Reader thread=id.clone() source=source open=open /> })}
        </div>
    }
}

#[derive(Clone, PartialEq)]
enum Reading {
    Loading,
    Text {
        text: String,
        next: Option<u64>,
        more: bool,
    },
    /// An object URL of the picture's bytes, once they came.
    Picture(Option<String>),
    Binary,
    Failed(String),
}

#[component]
fn Reader(thread: String, source: Source, open: RwSignal<Option<Source>>) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let reading = RwSignal::new(Reading::Loading);
    let dismiss = move || {
        if let Some(element) = dialog.get_untracked() {
            element.close();
        }
        open.set(None);
    };
    Effect::new(move |_| {
        if let Some(element) = dialog.get()
            && element.is_connected()
            && !element.open()
        {
            let _ = element.show_modal();
        }
    });
    let read = {
        let (thread, source) = (thread.clone(), source.clone());
        move |offset: u64| {
            let (thread, source) = (thread.clone(), source.clone());
            leptos::task::spawn_local(async move {
                match api::source_text(&thread, &source.source_id, offset).await {
                    Ok(chunk) => take(reading, chunk, &thread, &source),
                    Err(error) => reading.set(Reading::Failed(error)),
                }
            });
        }
    };
    read(0);
    // A picture's object URL lives as long as the window.
    on_cleanup(move || {
        if let Reading::Picture(Some(url)) = reading.get_untracked() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
    });
    let markdown = is_markdown(&source.name);
    let shown_size = size(source.bytes);
    let total = source.bytes;
    view! {
        <dialog class="project-dialog" node_ref=dialog aria-label=source.name.clone()
            on:click=move |_| dismiss()
            on:close=move |_| open.set(None)
            on:keydown=move |event| {
                event.stop_propagation();
                if event.key() == "Escape" {
                    event.prevent_default();
                    dismiss();
                }
            }>
            <div class="window source-window" on:click=|ev| ev.stop_propagation()>
                <div class="window-head">
                    <strong class="source-name">{source.name.clone()}</strong>
                    <span class="muted grow">{move || match reading.get() {
                        Reading::Text { .. } => format!("{shown_size} \u{00B7} text"),
                        Reading::Picture(_) => format!("{shown_size} \u{00B7} picture"),
                        Reading::Binary => format!("{shown_size} \u{00B7} binary file"),
                        Reading::Loading | Reading::Failed(_) => shown_size.clone(),
                    }}</span>
                    {move || match reading.get() {
                        Reading::Text { text, next, .. } => Some(view! {
                            <button class="ghost" title=if next.is_some() { "Copy the text shown so far" } else { "Copy the whole text" }
                                on:click=move |_| copy(&text)>
                                <Icon name=IconName::Copy />"Copy"
                            </button>
                        }),
                        _ => None,
                    }}
                    <button class="icon" title="Close" on:click=move |_| dismiss()>
                        <Icon name=IconName::Close />
                    </button>
                </div>
                <div class="viewer">
                {let read = read.clone(); move || match reading.get() {
                    Reading::Loading | Reading::Picture(None) => view! { <p class="muted">"Loading..."</p> }.into_any(),
                    Reading::Failed(error) => view! { <p class="muted">{error}</p> }.into_any(),
                    Reading::Binary => view! {
                        <p class="muted">"This file is not text, so it is not shown here. It stays on the machine running duskcode."</p>
                    }.into_any(),
                    Reading::Picture(Some(url)) => view! {
                        <img class="source-picture" src=url alt="" />
                    }.into_any(),
                    Reading::Text { text, next, more } => {
                        let long = text.len() >= MAX_SHOWN;
                        let shown = if markdown {
                            view! { <Markdown text=text /> }.into_any()
                        } else {
                            view! { <pre class="source-text">{text}</pre> }.into_any()
                        };
                        let read = read.clone();
                        let rest = next.map(move |next| {
                            let left = size(total.saturating_sub(next));
                            if long {
                                view! { <p class="muted">{format!("{left} more is not shown here.")}</p> }.into_any()
                            } else {
                                let read = read.clone();
                                view! {
                                    <button class="ghost" disabled=more on:click=move |_| {
                                        if let Reading::Text { more, .. } = &mut *reading.write() {
                                            *more = true;
                                        }
                                        read(next);
                                    }>{format!("Show more \u{00B7} {left} left")}</button>
                                }.into_any()
                            }
                        });
                        view! { {shown}{rest} }.into_any()
                    }
                }}
                </div>
            </div>
        </dialog>
    }
}

/// Puts a part the core sent into the reading; a picture's bytes are asked for next.
fn take(reading: RwSignal<Reading>, chunk: SourceChunk, thread: &str, source: &Source) {
    match chunk.kind {
        SourceKind::Binary => reading.set(Reading::Binary),
        SourceKind::Picture => {
            reading.set(Reading::Picture(None));
            let (thread, name, id) = (
                thread.to_owned(),
                source.name.clone(),
                source.source_id.clone(),
            );
            leptos::task::spawn_local(async move {
                // By id: names repeat, as every pasted screenshot is `image.png`.
                match api::source_picture(&thread, &name, Some(&id)).await {
                    Ok(bytes) => match object_url(&bytes) {
                        Some(url) => reading.set(Reading::Picture(Some(url))),
                        None => reading.set(Reading::Failed("cannot show the picture".into())),
                    },
                    Err(error) => reading.set(Reading::Failed(error)),
                }
            });
        }
        SourceKind::Text => reading.update(|reading| match reading {
            Reading::Text { text, next, more } => {
                text.push_str(&chunk.text);
                *next = chunk.next;
                *more = false;
            }
            _ => {
                *reading = Reading::Text {
                    text: chunk.text,
                    next: chunk.next,
                    more: false,
                }
            }
        }),
    }
}

/// An object URL of a PNG or JPEG the core checked, typed by its first bytes.
fn object_url(bytes: &[u8]) -> Option<String> {
    let kind = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    };
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(kind);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

/// Markdown is drawn; other text, code included, is shown as it is.
fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}
