//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use leptos::prelude::*;

#[component]
pub fn Editor(
    title: String,
    text: RwSignal<String>,
    save: impl Fn(String) + 'static,
    close: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <div class="editor">
            <h4>{title}</h4>
            <textarea rows="18" spellcheck="false" prop:value=move || text.get()
                on:input=move |ev| text.set(event_target_value(&ev))></textarea>
            <div class="row-actions">
                <button class="primary" on:click=move |_| save(text.get_untracked())>"Save"</button>
                <button class="ghost" on:click=move |_| close()>"Close"</button>
            </div>
        </div>
    }
}
