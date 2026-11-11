//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use super::editor::Editor;
use crate::{
    api,
    ui::{Switch, run},
};
use dusk_protocol::{catalog::AgentNode, client::Switch as Flag, config::Settings};
use leptos::prelude::*;

#[component]
pub fn Agents(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let open = RwSignal::new(None::<String>);
    let text = RwSignal::new(String::new());
    let edit = move |name: String| {
        let shown = name.clone();
        run(async move { api::agent(&name).await }, move |node| {
            text.set(node.text);
            open.set(Some(shown));
        });
    };
    view! {
        {move || panel.get().map(|p| {
            let (reload, edit) = (reload.clone(), edit);
            view! {
                {p.agents.problems.clone().into_iter().map(|problem| view! {
                    <p class="bad">{format!("{}: {}", problem.node, problem.reason)}</p>
                }).collect_view()}
                <ul class="agent-tree">{node_view(p.agents.root.clone(), reload, edit)}</ul>
            }
        })}
        {move || open.get().map(|name| {
            let title = format!("Agent {name}");
            let saved = name.clone();
            view! {
                <Editor title text
                    save=move |t| { let saved = saved.clone(); run(async move { api::save_agent(&saved, t).await }, |_| ()); }
                    close=move || open.set(None) />
            }
        })}
    }
}

pub fn node_view(
    node: AgentNode,
    reload: impl Fn() + Clone + Send + Sync + 'static,
    edit: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let name = node.name.clone();
    let (toggle, edit_name) = (name.clone(), name.clone());
    let children: Vec<_> = node
        .children
        .into_iter()
        .map(|c| node_view(c, reload.clone(), edit).into_any())
        .collect();
    let reload_toggle = reload.clone();
    view! {
        <li>
            <div class="node">
                <Switch on=node.enabled label=name.clone()
                    change=move |on| {
                        let (toggle, reload) = (toggle.clone(), reload_toggle.clone());
                        run(api::switch(Flag::Agent(toggle), on), move |()| reload());
                    } />
                <span class="muted">{node.description}</span>
                <button class="ghost" on:click=move |_| edit(edit_name.clone())>"Edit"</button>
            </div>
            {(!children.is_empty()).then(|| view! { <ul>{children}</ul> })}
        </li>
    }
}
