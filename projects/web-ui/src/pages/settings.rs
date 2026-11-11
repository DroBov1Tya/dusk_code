//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    sidebar::confirm,
    state::{AppState, View, state},
    theme::THEMES,
    ui::{Row, Section, Switch, run, size},
};
use dusk_protocol::{
    client::{Answer, Command, SettingsRequest, Switch as Flag},
    config::{ApprovalPolicy, ContextPolicy, ContextPreset, Profile, ProviderKind, Settings},
    model::Effort,
};
use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Appearance,
    Model,
    Chat,
    Safety,
    Secrets,
    AboutMe,
    Notifications,
    VersionControl,
    Memory,
    Advanced,
    Updates,
    Usage,
}

const PAGES: [(Page, &str); 12] = [
    (Page::Appearance, "Appearance"),
    (Page::Model, "Model"),
    (Page::Chat, "Chat"),
    (Page::Safety, "Safety"),
    (Page::Secrets, "Secrets"),
    (Page::AboutMe, "About me"),
    (Page::Notifications, "Notifications"),
    (Page::VersionControl, "Version control"),
    (Page::Memory, "Memory and context"),
    (Page::Advanced, "Advanced"),
    (Page::Updates, "Updates"),
    (Page::Usage, "System usage"),
];

#[component]
pub fn SettingsPage(page: Page) -> impl IntoView {
    let app = state();
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    view! {
        <div class="settings">
            <nav class="settings-nav">
                {PAGES.map(|(p, label)| view! {
                    <button class="nav-item" class:active=move || p == page
                        on:click=move |_| app.view.set(View::Settings(p))>{label}</button>
                })}
            </nav>
            <div class="page">
                {match page {
                    Page::Appearance => view! { <Appearance /> }.into_any(),
                    Page::Model => view! { <ModelPage /> }.into_any(),
                    Page::Chat => view! { <ChatPage panel reload /> }.into_any(),
                    Page::Safety => view! { <Safety panel reload /> }.into_any(),
                    Page::Secrets => view! { <Secrets /> }.into_any(),
                    Page::AboutMe => view! { <AboutMe /> }.into_any(),
                    Page::Notifications => view! { <Notifications panel reload /> }.into_any(),
                    Page::VersionControl => view! { <VersionControl panel reload /> }.into_any(),
                    Page::Memory => view! { <Memory panel reload /> }.into_any(),
                    Page::Advanced => view! { <Advanced panel reload /> }.into_any(),
                    Page::Updates => view! { <Updates /> }.into_any(),
                    Page::Usage => view! { <Usage /> }.into_any(),
                }}
            </div>
        </div>
    }
}

/// Sets a switch in the core, then loads the panel again.
fn flip(flag: Flag, on: bool, reload: impl Fn() + 'static) {
    run(api::switch(flag, on), move |()| reload());
}

#[component]
fn Appearance() -> impl IntoView {
    let app = state();
    view! {
        <h2>"Appearance"</h2>
        <p class="muted">"The theme is kept in this browser."</p>
        <Section title="Theme">
            <div class="themes">
                {THEMES.iter().map(|name| view! {
                    <button class="theme-card" class:active=move || app.theme.get() == *name
                        on:click=move |_| app.theme.set((*name).to_owned())>{*name}</button>
                }).collect_view()}
            </div>
        </Section>
    }
}

#[component]
fn ChatPage(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    view! {
        <h2>"Chat"</h2>
        <p class="muted">"How a new chat starts: its mode and what it may do."</p>
        {move || panel.get().map(|p| {
            let reload = reload.clone();
            view! {
                <Section title="Mode">
                    <select prop:value=p.mode.clone() on:change=move |ev| {
                        let (name, reload) = (event_target_value(&ev), reload.clone());
                        run(api::set(SettingsRequest::Mode(name)), move |()| reload());
                    }>
                        {p.modes.clone().into_iter().map(|m| view! {
                            <option value=m.name.clone() selected=m.name == p.mode>{format!("{} — {}", m.name, m.summary)}</option>
                        }).collect_view()}
                    </select>
                </Section>
                <ApprovalPicker current=p.approvals />
            }
        })}
    }
}

#[component]
fn ApprovalPicker(current: ApprovalPolicy) -> impl IntoView {
    let chosen = RwSignal::new(current);
    view! {
        <Section title="What agents may do without asking">
            {[
                (ApprovalPolicy::Ask, "Ask", "Every file outside the project and every command asks you."),
                (ApprovalPolicy::Sandbox, "Sandbox", "Commands run in a sandbox; the rest asks."),
                (ApprovalPolicy::Arbiter, "Arbiter", "A model decides in your place and explains."),
                (ApprovalPolicy::FullAccess, "Full access", "Nothing asks. Use only where a mistake is cheap."),
            ].map(|(policy, name, hint)| view! {
                <label class="option">
                    <input type="radio" name="approvals" prop:checked=move || chosen.get() == policy
                        on:change=move |_| run(api::switch(Flag::Approvals(policy), true), move |()| chosen.set(policy)) />
                    <span><b>{name}</b>" "<span class="muted">{hint}</span></span>
                </label>
            })}
        </Section>
    }
}

#[component]
fn Safety(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    let _ = reload;
    view! {
        <h2>"Safety"</h2>
        <p class="muted">"What the agents may do without asking."</p>
        {move || panel.get().map(|p| view! { <ApprovalPicker current=p.approvals /> })}
    }
}

#[component]
fn Notifications(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    view! {
        <h2>"Notifications"</h2>
        <p class="muted">"How you hear that a chat needs you. These apply to the desktop app and the terminal."</p>
        {move || panel.get().map(|p| {
            let (a, b) = (reload.clone(), reload.clone());
            view! {
                <Row name="Notifications" hint="A system notice when a chat needs you">
                    <Switch on=p.notifications change=move |on| flip(Flag::Notifications, on, a.clone()) />
                </Row>
                <Row name="Sounds" hint="A short sound with it">
                    <Switch on=p.sound_notifications change=move |on| flip(Flag::SoundNotifications, on, b.clone()) />
                </Row>
            }
        })}
    }
}

#[component]
fn VersionControl(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    view! {
        <h2>"Version control"</h2>
        <p class="muted">"Branches the agents create, and how they are named."</p>
        {move || panel.get().map(|p| {
            let reload = reload.clone();
            let prefix = RwSignal::new(p.version_control.branch_prefix.clone());
            view! {
                <Row name="Branch prefix" hint="Such as dusk/ ; empty adds none">
                    <input prop:value=move || prefix.get() on:input=move |ev| prefix.set(event_target_value(&ev)) />
                    <button class="ghost" on:click=move |_| {
                        let (value, reload) = (prefix.get_untracked(), reload.clone());
                        run(api::set(SettingsRequest::BranchPrefix(value)), move |()| reload());
                    }>"Save"</button>
                </Row>
            }
        })}
    }
}

fn number_row(
    name: &'static str,
    hint: &'static str,
    value: usize,
    save: impl Fn(usize) -> SettingsRequest + Clone + Send + Sync + 'static,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let text = RwSignal::new(value.to_string());
    view! {
        <Row name hint>
            <input type="number" min="0" prop:value=move || text.get() on:input=move |ev| text.set(event_target_value(&ev)) />
            <button class="ghost" on:click=move |_| {
                let Ok(number) = text.get_untracked().trim().parse::<usize>() else { return };
                let (request, reload) = (save(number), reload.clone());
                run(api::set(request), move |()| reload());
            }>"Save"</button>
        </Row>
    }
}

#[component]
fn Memory(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    let app = state();
    let _ = app;
    view! {
        <h2>"Memory and context"</h2>
        <p class="muted">"What is remembered between chats, and how much context is kept."</p>
        {move || panel.get().map(|p| {
            let (a, b, c, d) = (reload.clone(), reload.clone(), reload.clone(), reload.clone());
            view! {
                <Row name="Memory" hint="Notes the agents keep about you and your work">
                    <Switch on=p.memory.enabled change=move |on| flip(Flag::Memory, on, a.clone()) />
                </Row>
                {number_row("Memory size", "Characters of memory in each request", p.memory.max_chars, SettingsRequest::MemoryChars, b)}
                <Row name="Work history" hint="A short record of recent work in new chats">
                    <Switch on=p.work_history.enabled change=move |on| flip(Flag::WorkHistory, on, c.clone()) />
                </Row>
                {number_row("Work history size", "Characters of history", p.work_history.max_chars, SettingsRequest::WorkHistoryChars, d)}
                <ContextPicker />
            }
        })}
    }
}

#[component]
fn ContextPicker() -> impl IntoView {
    let app = state();
    let current = move || app.status.with(|s| s.as_ref().and_then(|s| s.context_policy));
    let pick = move |ev| {
        let value = event_target_value(&ev);
        let policy = match value.as_str() {
            "auto" => ContextPolicy::Auto,
            other => match ContextPreset::ALL.iter().find(|p| p.name() == other) {
                Some(preset) => ContextPolicy::Fixed(*preset),
                None => return,
            },
        };
        run(api::number(SettingsRequest::ContextPolicy(policy)), |_| ());
    };
    let selected = move || match current() {
        Some(ContextPolicy::Fixed(p)) => p.name().to_owned(),
        Some(ContextPolicy::Manual(n)) => format!("manual {n}"),
        _ => "auto".to_owned(),
    };
    view! {
        <Row name="Context size" hint="The largest request to the model">
            <select prop:value=selected on:change=pick>
                <option value="auto">"Auto (the model's window)"</option>
                {ContextPreset::ALL.map(|p| view! { <option value=p.name()>{p.name()}</option> })}
            </select>
        </Row>
    }
}

#[component]
fn Advanced(panel: RwSignal<Option<Settings>>, reload: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    view! {
        <h2>"Advanced"</h2>
        <p class="muted">"Limits and timings. The defaults suit most work."</p>
        {move || panel.get().map(|p| {
            let (a, b, c, d, e) = (reload.clone(), reload.clone(), reload.clone(), reload.clone(), reload.clone());
            view! {
                {number_row("Agents at work", "How many subagents run at once", p.max_agents_at_work, SettingsRequest::MaxAgentsAtWork, a)}
                {number_row("Goal rounds", "Turns a standing goal may take", p.max_goal_rounds as usize, |n| SettingsRequest::MaxGoalRounds(n as u32), b)}
                {number_row("Idle screen (minutes)", "Before the idle screen of the desktop app", p.idle_screen_minutes as usize, |n| SettingsRequest::IdleScreenMinutes(n as u32), c)}
                <Row name="Logging" hint="Write logs to the logs folder">
                    <Switch on=p.logging change=move |on| flip(Flag::Logging, on, d.clone()) />
                </Row>
                <Row name="Dev channel" hint="Updates from the development channel">
                    <Switch on=p.update_channel == dusk_protocol::release::Channel::Dev
                        change=move |on| flip(Flag::DevChannel, on, e.clone()) />
                </Row>
            }
        })}
    }
}

#[component]
fn ModelPage() -> impl IntoView {
    let app = state();
    let providers = RwSignal::new(Vec::new());
    let current = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let load = move || {
        busy.set(true);
        run(api::command(Command::Model { name: None }), move |answer| {
            busy.set(false);
            if let Answer::Providers { current: now, providers: list } = answer {
                current.set(now);
                providers.set(list);
            }
        });
    };
    load();
    let effort = move || app.status.with(|s| s.as_ref().and_then(|s| s.effort));
    view! {
        <h2>"Model"</h2>
        <p class="muted">"The models the chat can use, and the servers that give them."</p>
        <Section title="Reasoning effort">
            <select prop:value=move || effort().map(|e| e.name()).unwrap_or("default") on:change=move |ev| {
                let level = event_target_value(&ev);
                run(api::command(Command::Effort { level: Some(level) }), |_| ());
            }>
                {Effort::ALL.map(|e| view! { <option value=e.name()>{e.name()}</option> })}
            </select>
        </Section>
        <Section title="Providers">
            <Show when=move || busy.get()><p class="muted">"Checking the providers…"</p></Show>
            {move || providers.get().into_iter().map(|provider| {
                let name = provider.name.clone();
                let removing = name.clone();
                view! {
                    <div class="provider">
                        <div class="provider-head">
                            <b>{name.clone()}</b>
                            {provider.balance.as_ref().and_then(|b| b.as_ref().ok()).map(|b| view! { <span class="muted">{b.text()}</span> })}
                            <span class="grow"></span>
                            <button class="ghost danger" on:click=move |_| {
                                if confirm("Remove this provider and its key?") {
                                    let name = removing.clone();
                                    run(api::set(SettingsRequest::RemoveProvider { name }), move |()| load());
                                }
                            }>"Remove"</button>
                        </div>
                        {match &provider.models {
                            Err(error) => view! { <p class="bad">{error.clone()}</p> }.into_any(),
                            Ok(models) => models.iter().map(|model| {
                                let full = format!("{}/{}", provider.name, model.id);
                                let active = current.get_untracked().as_deref() == Some(full.as_str());
                                let pick = full.clone();
                                view! {
                                    <button class="model" class:active=active on:click=move |_| {
                                        let pick = pick.clone();
                                        run(api::command(Command::Model { name: Some(pick) }), move |_| load());
                                    }>
                                        <span>{model.id.clone()}</span>
                                        <span class="muted">{model.context_window.map(crate::ui::compact).unwrap_or_default()}</span>
                                    </button>
                                }
                            }).collect_view().into_any(),
                        }}
                    </div>
                }
            }).collect_view()}
        </Section>
        <ConnectProvider done=load />
    }
}

#[component]
fn ConnectProvider(done: impl Fn() + Clone + Send + Sync + 'static) -> impl IntoView {
    let kind = RwSignal::new(ProviderKind::OpenAiCompatible);
    let (name, base, key) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
    let connect = move |_| {
        let api_key = Some(key.get_untracked()).filter(|k| !k.is_empty()).map(dusk_protocol::client::Secret);
        let request = SettingsRequest::Connect {
            kind: kind.get_untracked(),
            name: Some(name.get_untracked()).filter(|n| !n.is_empty()),
            base_url: base.get_untracked(),
            api_key,
        };
        let done = done.clone();
        run(api::connect_provider(request), move |_| {
            key.set(String::new());
            done();
        });
    };
    view! {
        <Section title="Connect a provider">
            <select on:change=move |ev| kind.set(match event_target_value(&ev).as_str() {
                "openai" => ProviderKind::OpenAi,
                "anthropic" => ProviderKind::Anthropic,
                _ => ProviderKind::OpenAiCompatible,
            })>
                <option value="openai_compatible">"OpenAI-compatible"</option>
                <option value="openai">"OpenAI"</option>
                <option value="anthropic">"Anthropic"</option>
            </select>
            <input placeholder="Name (optional)" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
            <input placeholder=move || kind.get().base_url().unwrap_or("Base URL, such as http://localhost:11434/v1")
                prop:value=move || base.get() on:input=move |ev| base.set(event_target_value(&ev)) />
            <input type="password" autocomplete="off" placeholder="API key (stays in the keychain)"
                prop:value=move || key.get() on:input=move |ev| key.set(event_target_value(&ev)) />
            <button class="primary" on:click=connect>"Connect"</button>
        </Section>
    }
}

/// Keys and tokens: whether each is set, never its value.
#[component]
fn Secrets() -> impl IntoView {
    let info = RwSignal::new(None::<dusk_protocol::config::SecretsInfo>);
    let load = move || run(api::secrets(), move |s| info.set(Some(s)));
    load();
    view! {
        <h2>"Secrets"</h2>
        <p class="muted">"Keys and tokens, kept in the system keychain. Values are never shown."</p>
        {move || info.get().map(|info| view! {
            {info.problem.clone().map(|p| view! { <p class="bad">{p}</p> })}
            {info.entries.clone().into_iter().map(|entry| {
                let value = RwSignal::new(String::new());
                let (set_name, delete_name) = (entry.name.clone(), entry.name.clone());
                view! {
                    <Row name=entry.label.clone() hint=if entry.set { "set" } else { "not set" }>
                        {entry.editable.then(|| view! {
                            <input type="password" autocomplete="off" placeholder="new value"
                                prop:value=move || value.get() on:input=move |ev| value.set(event_target_value(&ev)) />
                            <button class="ghost" on:click=move |_| {
                                let (name, v) = (set_name.clone(), value.get_untracked());
                                run(api::set_secret(name, v), move |()| { value.set(String::new()); load(); });
                            }>"Save"</button>
                            <button class="ghost danger" on:click=move |_| {
                                let name = delete_name.clone();
                                run(api::set(SettingsRequest::DeleteSecret { name }), move |()| load());
                            }>"Delete"</button>
                        })}
                    </Row>
                }
            }).collect_view()}
        })}
    }
}

#[component]
fn AboutMe() -> impl IntoView {
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

#[component]
fn Updates() -> impl IntoView {
    let app = state();
    let info = RwSignal::new(None::<dusk_protocol::release::UpdateInfo>);
    let check = move || run(api::command(Command::Update { channel: None }), move |a| {
        if let Answer::Update(u) = a {
            info.set(Some(u));
        }
    });
    check();
    let progress = move || app.status.with(|s| s.as_ref().and_then(|s| s.install));
    view! {
        <h2>"Updates"</h2>
        <p class="muted">"The version you run and the newest one."</p>
        {move || info.get().map(|u| {
            let tag = u.latest.clone();
            view! {
                <Row name="Running" hint=u.current.clone()><span></span></Row>
                <Row name="Newest" hint=format!("{}{}", u.latest, if u.prerelease { " (pre-release)" } else { "" })>
                    {(u.newer && u.ready).then(|| view! {
                        <button class="primary" on:click=move |_| {
                            let tag = tag.clone();
                            run(api::install_update(tag), move |path| app.fail(format!("Installed: {path}")));
                        }>"Install"</button>
                    })}
                </Row>
            }
        })}
        {move || progress().map(|p| view! {
            <p>{format!("{:?}: {} of {}", p.stage, size(p.downloaded), p.total.map(size).unwrap_or_else(|| "?".into()))}</p>
        })}
        <button class="ghost" on:click=move |_| check()>"Check again"</button>
    }
}

#[component]
fn Usage() -> impl IntoView {
    let report = RwSignal::new(None);
    run(api::system_report(), move |r| report.set(Some(r)));
    view! {
        <h2>"System usage"</h2>
        <p class="muted">"What duskcode takes from the machine it runs on."</p>
        {move || report.get().map(|r: dusk_protocol::system::SystemReport| view! {
            {r.process.map(|p| view! {
                <Row name="Memory" hint=""><span>{size(p.resident_bytes)}</span></Row>
                <Row name="CPU" hint=""><span>{format!("{:.1}%", p.cpu_percent)}</span></Row>
            })}
            <Row name="Database" hint=""><span>{size(r.database_bytes + r.wal_bytes)}</span></Row>
            <Row name="Artifacts" hint=""><span>{size(r.artifacts_bytes)}</span></Row>
            <Row name="Logs" hint=""><span>{size(r.logs_bytes)}</span></Row>
            <Row name="Chats" hint=""><span>{format!("{} threads, {} messages", r.threads, r.messages)}</span></Row>
        })}
    }
}

#[allow(dead_code)]
fn _unused(_: AppState) {}
