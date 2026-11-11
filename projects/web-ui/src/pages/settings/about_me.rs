//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Section, run},
};
use dusk_protocol::config::Profile;
use leptos::prelude::*;

#[component]
pub fn AboutMe() -> impl IntoView {
    let about = RwSignal::new(String::new());
    let profile = RwSignal::new(Profile::default());
    run(api::profile(), move |p| {
        about.set(p.about.clone());
        profile.set(p);
    });
    let snippets = RwSignal::new(Vec::new());
    let load = move || run(api::snippets(), move |s| snippets.set(s));
    load();
    let (new_name, new_text) = (RwSignal::new(String::new()), RwSignal::new(String::new()));
    view! {
        <h2>"About me"</h2>
        <p class="muted">"What the agents know about you, and snippets for @names."</p>
        <Section title="About me (@me)">
            <textarea rows="6" prop:value=move || about.get() on:input=move |ev| about.set(event_target_value(&ev))></textarea>
            <button class="primary" on:click=move |_| {
                let mut next = profile.get_untracked();
                next.about = about.get_untracked();
                run(api::save_profile(next.clone()), move |()| profile.set(next));
            }>"Save"</button>
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
