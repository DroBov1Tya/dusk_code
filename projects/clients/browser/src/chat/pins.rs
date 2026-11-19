//! The pins of the chat, as the desktop app has them: a thin handle on the transcript's
//! upper edge says how many there are, and a click pulls a drawer of them down over the
//! chat. Excerpts are the core's plain text.

use crate::{
    api,
    state::Chat,
    ui::{Icon, IconName, run},
};
use dusk_protocol::thread::MessageRole;
use leptos::prelude::*;

#[component]
pub fn Pins(id: String, chat: Chat) -> impl IntoView {
    let open = RwSignal::new(false);
    let asked = id.clone();
    run(async move { api::pins(&asked).await }, move |pins| {
        chat.update(|c| c.pins = pins)
    });
    let count = move || chat.with(|c| c.pins.len());
    let id = StoredValue::new(id);
    view! {
        <Show when=move || count() != 0>
            <div class="pins">
                <button class="pins-handle" aria-expanded=move || open.get().to_string()
                    aria-label=move || if open.get() { "Fold the pinned messages" } else { "Show the pinned messages" }
                    on:click=move |_| open.update(|o| *o = !*o)>
                    <span>{move || format!("{} pinned", count())}</span>
                    <Icon name=IconName::ChevronDown />
                </button>
                <Show when=move || open.get()>
                    <ul class="pins-drawer">
                        {move || chat.with(|c| c.pins.clone()).into_iter().map(|pin| {
                            let who = match pin.role {
                                MessageRole::User => "You",
                                _ => "Agent",
                            };
                            let message_id = pin.message_id.clone();
                            view! {
                                <li class="pin-row">
                                    <span class="pin-who muted">{who}</span>
                                    <span class="pin-text" class:muted=pin.compacted
                                        title=if pin.compacted { "Folded away by a compaction" } else { "" }>
                                        {pin.excerpt.to_string()}
                                    </span>
                                    <button class="icon pin-unpin" title="Unpin" aria-label="Unpin" on:click=move |_| {
                                        let (thread_id, message_id) = (id.get_value(), message_id.clone());
                                        run(async move { api::set_pinned(&thread_id, &message_id, false).await },
                                            move |pins| chat.update(|c| c.pins = pins));
                                    }><Icon name=IconName::Close /></button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                </Show>
            </div>
        </Show>
    }
}
