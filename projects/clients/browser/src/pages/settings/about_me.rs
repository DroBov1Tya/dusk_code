//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Section, run},
};
use dusk_protocol::config::Profile;
use leptos::prelude::*;

/// What the last save of the profile came to, said beside its button until the text changes.
#[derive(Clone)]
enum Save {
    Quiet,
    Saved,
    Failed(String),
}

#[component]
pub fn AboutMe() -> impl IntoView {
    let about = RwSignal::new(String::new());
    let profile = RwSignal::new(Profile::default());
    let loaded = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let save = RwSignal::new(Save::Quiet);
    let load_profile = move || {
        if loading.get_untracked() {
            return;
        }
        loading.set(true);
        leptos::task::spawn_local(async move {
            match api::profile().await {
                Ok(p) => {
                    about.set(p.about.clone());
                    profile.set(p);
                    loaded.set(true);
                    save.set(Save::Quiet);
                }
                Err(error) => save.set(Save::Failed(format!("Could not load: {error}"))),
            }
            loading.set(false);
        });
    };
    load_profile();
    let snippets = RwSignal::new(Vec::new());
    let load = move || run(api::snippets(), move |s| snippets.set(s));
    load();
    let (new_name, new_text) = (RwSignal::new(String::new()), RwSignal::new(String::new()));
    view! {
        <h2>"About me"</h2>
        <p class="muted">"What the agents know about you, and snippets for @names."</p>
        <Section title="About me (@me)">
            <textarea rows="6" disabled=move || !loaded.get() prop:value=move || about.get() on:input=move |ev| { about.set(event_target_value(&ev)); save.set(Save::Quiet); }></textarea>
            <Show when=move || !loaded.get() && !loading.get()>
                <button class="ghost" on:click=move |_| load_profile()>"Retry loading"</button>
            </Show>
            <button class="primary" disabled=move || !loaded.get() || saving.get() on:click=move |_| {
                if saving.get_untracked() || !loaded.get_untracked() { return; }
                saving.set(true);
                save.set(Save::Quiet);
                let mut next = profile.get_untracked();
                let submitted = about.get_untracked();
                next.about = submitted.clone();
                leptos::task::spawn_local(async move {
                    let result = match api::save_profile(next).await {
                        Ok(()) => api::profile().await.map_err(|error| format!("Could not confirm save: {error}")),
                        Err(error) => Err(format!("Not saved: {error}")),
                    };
                    match result {
                        Ok(p) => {
                            if about.get_untracked() == submitted {
                                about.set(p.about.clone());
                            }
                            profile.set(p);
                            save.set(Save::Saved);
                        }
                        Err(error) => save.set(Save::Failed(error)),
                    }
                    saving.set(false);
                });
            }>"Save"</button>
            {move || match save.get() {
                _ if loading.get() => view! { <span class="muted">" Loading…"</span> }.into_any(),
                _ if saving.get() => view! { <span class="muted">" Saving…"</span> }.into_any(),
                Save::Failed(error) => view! { <span class="error">{format!(" {error}")}</span> }.into_any(),
                _ if loaded.get() && about.get() != profile.get().about => view! { <span class="muted">" Unsaved changes"</span> }.into_any(),
                Save::Saved => view! { <span class="muted">" \u{2713} Saved"</span> }.into_any(),
                Save::Quiet => ().into_any(),
            }}
        </Section>
        <Section title="Snippets">
            {move || snippets.get().into_iter().map(|s| view! {
                <div class="snippet"><b>{format!("@{}", s.name)}</b><pre>{s.text.clone()}</pre></div>
            }).collect_view()}
            <input placeholder="name" prop:value=move || new_name.get() on:input=move |ev| new_name.set(event_target_value(&ev)) />
            <textarea rows="3" placeholder="text" prop:value=move || new_text.get() on:input=move |ev| new_text.set(event_target_value(&ev))></textarea>
            <button class="ghost" on:click=move |_| {
                let (n, t) = (new_name.get_untracked(), new_text.get_untracked());
                run(api::save_snippet(n, t), move |()| { new_name.set(String::new()); new_text.set(String::new()); load(); });
            }>"Save snippet"</button>
        </Section>
    }
}
