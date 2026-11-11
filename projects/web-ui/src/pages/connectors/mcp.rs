//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use super::editor::Editor;
use crate::{
    api,
    state::state,
    ui::{Switch, run},
};
use dusk_protocol::{
    client::Switch as Flag,
    config::{McpInfo, McpState},
};
use leptos::prelude::*;

#[component]
pub fn Mcp() -> impl IntoView {
    let servers = RwSignal::new(Vec::<McpInfo>::new());
    let load = move || run(api::mcp_servers(), move |s| servers.set(s));
    load();
    let editing = RwSignal::new(None::<Option<String>>);
    let text = RwSignal::new(String::new());
    let add = move |_| {
        text.set("{\n  \"name\": \"\",\n  \"command\": \"\",\n  \"args\": []\n}".to_owned());
        editing.set(Some(None));
    };
    view! {
        <div class="row-actions"><button class="primary" on:click=add>"Add a server"</button></div>
        <div class="cards">
            {move || servers.get().into_iter().map(|server| {
                let (toggle, restart, sign_in, _edit) = (server.name.clone(), server.name.clone(), server.name.clone(), server.name.clone());
                let (state_text, failed) = match &server.state {
                    McpState::Off => ("off".to_owned(), false),
                    McpState::Starting => ("starting".to_owned(), false),
                    McpState::Ready(tools) => (format!("ready · {} tools", tools.len()), false),
                    McpState::Failed(why) => (why.clone(), true),
                };
                view! {
                    <div class="card-tile">
                        <div class="tile-head"><b>{server.name.clone()}</b>
                            <span class="chip">{if server.remote { "remote" } else { "local" }}</span></div>
                        <p class="muted">{server.target.clone()}</p>
                        <p class:bad=failed>{state_text}</p>
                        <details><summary>{format!("{} tools", server.tools.len())}</summary>
                            {server.tools.clone().into_iter().map(|t| {
                                let tool = t.name.clone();
                                view! { <Switch on=t.enabled label=t.name.clone() change=move |on| {
                                    let tool = tool.clone();
                                    run(api::switch(Flag::Tool(tool), on), |()| ());
                                } /> }
                            }).collect_view()}
                        </details>
                        <div class="row-actions">
                            <Switch on=!matches!(server.state, McpState::Off) change=move |on| {
                                let toggle = toggle.clone();
                                run(api::switch(Flag::Mcp(toggle), on), move |()| load());
                            } />
                            <button class="ghost" on:click=move |_| {
                                let name = restart.clone();
                                run(async move { api::restart_mcp(&name).await }, move |()| load());
                            }>"Restart"</button>
                            {server.remote.then(|| view! { <button class="ghost" on:click=move |_| {
                                let name = sign_in.clone();
                                run(async move { api::sign_in_mcp(&name).await }, move |msg| state().fail(msg));
                            }>"Sign in"</button> })}
                        </div>
                    </div>
                }
            }).collect_view()}
        </div>
        {move || editing.get().map(|old| view! {
            <Editor title=old.clone().unwrap_or_else(|| "New server".to_owned()) text
                save=move |t| { let old = old.clone(); run(api::save_mcp_server(old, t), move |_| load()); }
                close=move || editing.set(None) />
        })}
    }
}
