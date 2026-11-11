//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use crate::{
    api,
    sidebar::confirm,
    state::{View, state},
    ui::{Row, Section, Switch, run, sleep},
};
use dusk_protocol::{
    catalog::AgentNode,
    client::{SettingsRequest, Switch as Flag, VoiceRequest},
    config::{
        LocalState, McpInfo, McpState, Settings, SpeechKind, ToolKind, VoiceEndpoint, VoiceModels,
        VoiceSettings,
    },
};
use leptos::{prelude::*, task::spawn_local};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Agents,
    Skills,
    Mcp,
    Plugins,
    Tools,
    Voice,
}

const TABS: [(Tab, &str); 6] = [
    (Tab::Agents, "Agents"),
    (Tab::Skills, "Skills"),
    (Tab::Mcp, "MCP"),
    (Tab::Plugins, "Plugins"),
    (Tab::Tools, "Tools"),
    (Tab::Voice, "Voice"),
];

#[component]
pub fn Connectors(tab: Tab) -> impl IntoView {
    let app = state();
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    view! {
        <div class="page">
            <h2>"Connectors"</h2>
            <div class="tabs">
                {TABS.map(|(t, label)| view! {
                    <button class="tab" class:active=move || t == tab
                        on:click=move |_| app.view.set(View::Connectors(t))>{label}</button>
                })}
            </div>
            {match tab {
                Tab::Agents => view! { <Agents panel reload /> }.into_any(),
                Tab::Skills => view! { <Skills panel reload /> }.into_any(),
                Tab::Mcp => view! { <Mcp /> }.into_any(),
                Tab::Plugins => view! { <Plugins /> }.into_any(),
                Tab::Tools => view! { <Tools panel reload /> }.into_any(),
                Tab::Voice => view! { <Voice /> }.into_any(),
            }}
        </div>
    }
}

/// A text file of the catalog open for editing.
#[component]
fn Editor(
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

#[component]
fn Agents(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
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

fn node_view(
    node: AgentNode,
    reload: impl Fn() + Clone + Send + Sync + 'static,
    edit: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let name = node.name.clone();
    let (toggle, edit_name) = (name.clone(), name.clone());
    let children: Vec<_> = node.children.into_iter().map(|c| node_view(c, reload.clone(), edit).into_any()).collect();
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

#[component]
fn Skills(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
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

#[component]
fn Mcp() -> impl IntoView {
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
                let (toggle, restart, sign_in, edit) = (server.name.clone(), server.name.clone(), server.name.clone(), server.name.clone());
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
                            <button class="ghost" on:click=move |_| {
                                let name = edit.clone();
                                let shown = name.clone();
                                run(async move { api::mcp_server_text(&name).await }, move |t| { text.set(t); editing.set(Some(Some(shown))); });
                            }>"Edit"</button>
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

#[component]
fn Plugins() -> impl IntoView {
    let app = state();
    view! {
        <Show when=move || app.status.with(|s| s.as_ref().is_some_and(|s| !s.plugins.is_empty()))
            fallback=|| view! { <p class="muted">"No plugins are installed."</p> }>
            <div class="cards">{move || app.status.get().map(|s| s.plugins).unwrap_or_default().into_iter().map(|plugin| {
                let toggle = plugin.name.clone();
                view! {
                    <div class="card-tile">
                        <div class="tile-head"><b>{plugin.name.clone()}</b><span class="muted">{plugin.version.clone()}</span></div>
                        <p class="muted">{plugin.description.clone()}</p>
                        {plugin.problem.clone().map(|p| view! { <p class="bad">{p}</p> })}
                        {plugin.missing_settings.clone().into_iter().map(|m| view! { <p class="bad">{format!("Missing setting: {m}")}</p> }).collect_view()}
                        {plugin.secrets.clone().into_iter().map(|(key, set)| {
                            let (name, key_name, value) = (plugin.name.clone(), key.clone(), RwSignal::new(String::new()));
                            view! {
                                <div class="row-actions">
                                    <span>{format!("{key} ({})", if set { "set" } else { "not set" })}</span>
                                    <input type="password" autocomplete="off" prop:value=move || value.get()
                                        on:input=move |ev| value.set(event_target_value(&ev)) />
                                    <button class="ghost" on:click=move |_| {
                                        let request = SettingsRequest::PluginSecret {
                                            plugin: name.clone(), key: key_name.clone(),
                                            value: dusk_protocol::client::Secret(value.get_untracked()),
                                        };
                                        run(api::set(request), move |()| value.set(String::new()));
                                    }>"Save"</button>
                                </div>
                            }
                        }).collect_view()}
                        <Switch on=plugin.enabled label="Enabled" change=move |on| {
                            let request = SettingsRequest::PluginEnabled { name: toggle.clone(), enabled: on };
                            run(api::set(request), |()| ());
                        } />
                    </div>
                }
            }).collect_view()}</div>
        </Show>
    }
}

/// Tools by kind. Computer use is the desktop app's, and has no switch here.
#[component]
fn Tools(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    view! {
        {move || panel.get().map(|p| {
            let reload = reload.clone();
            [ToolKind::Files, ToolKind::Console, ToolKind::Agents, ToolKind::Harness, ToolKind::Mcp].map(|kind| {
                let tools: Vec<_> = p.tools.iter().filter(|t| t.kind == kind).cloned().collect();
                let reload = reload.clone();
                (!tools.is_empty()).then(|| view! {
                    <Section title=kind.name()>
                        {tools.into_iter().map(|tool| {
                            let (name, reload) = (tool.name.clone(), reload.clone());
                            view! {
                                <Row name=tool.name.clone() hint=tool.description.clone()>
                                    <Switch on=tool.enabled change=move |on| {
                                        let (name, reload) = (name.clone(), reload.clone());
                                        run(api::switch(Flag::Tool(name), on), move |()| reload());
                                    } />
                                </Row>
                            }
                        }).collect_view()}
                    </Section>
                })
            })
        })}
    }
}

#[component]
fn Voice() -> impl IntoView {
    let models = RwSignal::new(VoiceModels::default());
    let settings = RwSignal::new(VoiceSettings::default());
    let load = move || {
        run(api::voice_models(), move |m| models.set(m));
        run(api::voice_settings(), move |s| settings.set(s));
    };
    load();
    // Installing runs on its own; keep asking while any model installs.
    spawn_local(async move {
        loop {
            sleep(2000).await;
            if models.with_untracked(|m| m.models.iter().any(|m| matches!(m.state, LocalState::Installing(_)))) {
                if let Ok(next) = api::voice_models().await {
                    models.set(next);
                }
            }
        }
    });
    view! {
        <p class="muted">"Speech models run on the machine that runs duskcode, installed with uv."</p>
        <Show when=move || !models.with(|m| m.uv_found)>
            <p class="bad">"uv was not found. Install it to set up local speech models."</p>
        </Show>
        <div class="cards">
            {move || models.get().models.into_iter().map(|model| {
                let id = model.id.clone();
                let (install, cancel, remove) = (id.clone(), id.clone(), id.clone());
                let kind = match model.kind { SpeechKind::Recognition => "speech to text", SpeechKind::Synthesis => "text to speech" };
                let (label, installing) = match &model.state {
                    LocalState::NotInstalled => ("not installed".to_owned(), false),
                    LocalState::Installing(step) => (format!("installing: {step}"), true),
                    LocalState::Installed => ("installed".to_owned(), false),
                    LocalState::Running => ("running".to_owned(), false),
                    LocalState::Failed(why) => (format!("failed: {why}"), false),
                };
                let installed = matches!(model.state, LocalState::Installed | LocalState::Running);
                view! {
                    <div class="card-tile">
                        <div class="tile-head"><b>{model.title.clone()}</b><span class="chip">{kind}</span></div>
                        <p class="muted">{model.about.clone()}</p>
                        <p class="muted">{format!("{} · {}", model.size, model.languages)}</p>
                        <p>{label}{model.in_use.then_some(" · in use")}</p>
                        <div class="row-actions">
                            {(!installed && !installing).then(|| view! {
                                <button class="primary" on:click=move |_| {
                                    let id = install.clone();
                                    run(api::voice(VoiceRequest::Install { id }), move |()| load());
                                }>"Install"</button>
                            })}
                            {installing.then(|| view! {
                                <button class="ghost" on:click=move |_| {
                                    let id = cancel.clone();
                                    run(api::voice(VoiceRequest::Cancel { id }), move |()| load());
                                }>"Cancel"</button>
                            })}
                            {installed.then(|| view! {
                                <button class="ghost danger" on:click=move |_| {
                                    if confirm("Remove this model from the machine?") {
                                        let id = remove.clone();
                                        run(api::voice(VoiceRequest::Remove { id }), move |()| load());
                                    }
                                }>"Remove"</button>
                            })}
                        </div>
                    </div>
                }
            }).collect_view()}
        </div>
        <Section title="Other servers">
            <p class="muted">"Point speech at any OpenAI-compatible server instead."</p>
            <EndpointForm title="Speech to text" settings kind=SpeechKind::Recognition />
            <EndpointForm title="Text to speech" settings kind=SpeechKind::Synthesis />
        </Section>
    }
}

#[component]
fn EndpointForm(title: &'static str, settings: RwSignal<VoiceSettings>, kind: SpeechKind) -> impl IntoView {
    let read = move |s: &VoiceSettings| match kind {
        SpeechKind::Recognition => s.stt.clone(),
        SpeechKind::Synthesis => s.tts.clone(),
    }
    .unwrap_or_default();
    let (url, model, voice) = (RwSignal::new(String::new()), RwSignal::new(String::new()), RwSignal::new(String::new()));
    Effect::new(move |_| {
        let endpoint = settings.with(read);
        url.set(endpoint.base_url);
        model.set(endpoint.model);
        voice.set(endpoint.voice.unwrap_or_default());
    });
    let save = move |_| {
        let endpoint = Some(VoiceEndpoint {
            base_url: url.get_untracked(),
            model: model.get_untracked(),
            voice: Some(voice.get_untracked()).filter(|v| !v.is_empty()),
        })
        .filter(|e| !e.base_url.is_empty());
        let mut next = settings.get_untracked();
        match kind {
            SpeechKind::Recognition => next.stt = endpoint,
            SpeechKind::Synthesis => next.tts = endpoint,
        }
        run(api::set(SettingsRequest::Voice(next.clone())), move |()| settings.set(next));
    };
    view! {
        <Row name=title hint="Base URL, model and, for speech, a voice">
            <input placeholder="http://localhost:8000/v1" prop:value=move || url.get() on:input=move |ev| url.set(event_target_value(&ev)) />
            <input placeholder="model" prop:value=move || model.get() on:input=move |ev| model.set(event_target_value(&ev)) />
            {(kind == SpeechKind::Synthesis).then(|| view! {
                <input placeholder="voice" prop:value=move || voice.get() on:input=move |ev| voice.set(event_target_value(&ev)) />
            })}
            <button class="ghost" on:click=save>"Save"</button>
        </Row>
    }
}
