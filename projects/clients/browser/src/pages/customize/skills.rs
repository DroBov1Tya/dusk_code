//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use super::editor::Editor;
use crate::{
    api,
    ui::{Switch, run},
};
use dusk_protocol::{client::Switch as Flag, config::Settings};
use leptos::prelude::*;

#[component]
pub fn Skills(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let open = RwSignal::new(None::<String>);
    let text = RwSignal::new(String::new());
    view! {
        {move || panel.get().map(|p| {
            let reload = reload.clone();
            view! {
                <div class="cards">{p.skills.clone().into_iter().map(|skill| {
                    let (toggle, edit) = (skill.name.clone(), skill.name.clone());
                    let reload = reload.clone();
                    view! {
                        <div class="card-tile">
                            <div class="tile-head"><b>{skill.name.clone()}</b><span class="chip">{skill.kind.name()}</span></div>
                            <p class="muted">{skill.description.clone()}</p>
                            {skill.problem.clone().map(|p| view! { <p class="bad">{p}</p> })}
                            <div class="row-actions">
                                <Switch on=skill.enabled change=move |on| {
                                    let (toggle, reload) = (toggle.clone(), reload.clone());
                                    run(api::switch(Flag::Skill(toggle), on), move |()| reload());
                                } />
                                <button class="ghost" on:click=move |_| {
                                    let name = edit.clone();
                                    let shown = name.clone();
                                    run(async move { api::skill(&name).await }, move |node| { text.set(node.text); open.set(Some(shown)); });
                                }>"Edit"</button>
                            </div>
                        </div>
                    }
                }).collect_view()}</div>
            }
        })}
        {move || open.get().map(|name| {
            let saved = name.clone();
            view! {
                <Editor title=format!("Skill {name}") text
                    save=move |t| { let saved = saved.clone(); run(async move { api::save_skill(&saved, t).await }, |_| ()); }
                    close=move || open.set(None) />
            }
        })}
    }
}
