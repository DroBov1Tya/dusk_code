//! The Tools page: the tools that stay off until the user switches them on, each with
//! what it does and what it risks, then the ways to add more.

use crate::{
    api,
    pages::customize::Tab,
    state::{View, state},
    ui::{Row, Switch, run},
};
use dusk_protocol::{
    client::Switch as Flag,
    config::{Settings, ToolInfo, ToolKind},
};
use leptos::{prelude::*, task::spawn_local};

/// Moves the user's real pointer: its own switch, apart from the rest of computer use.
const REAL_POINTER: &str = "physical_input";

#[component]
pub fn Tools() -> impl IntoView {
    let panel = RwSignal::new(None::<Settings>);
    let reload = move || run(api::settings_panel(), move |p| panel.set(Some(p)));
    reload();
    let app = state();
    view! {
        <div class="welcome-page">
            <h2>"Tools"</h2>
            <p class="muted">"Most tools are on already. These stay off until you switch them on."</p>
            {move || panel.get().map(|p| offered(p.tools, reload))}
            <div class="welcome-links">
                <button class="welcome-link" on:click=move |_| app.show(View::Customize(Tab::Voice))>
                    <b>"Voice"</b>
                    <span class="muted">"Talk to the agent and hear it answer. Needs speech servers set up."</span>
                </button>
                <button class="welcome-link" on:click=move |_| app.show(View::Customize(Tab::Mcp))>
                    <b>"Workshop"</b>
                    <span class="muted">"Connectors and plugins: add services and apps the agent can use."</span>
                </button>
            </div>
        </div>
    }
}

/// The switches, then a quiet count of the tools already on.
fn offered(
    tools: Vec<ToolInfo>,
    reload: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let screen: Vec<_> = tools
        .iter()
        .filter(|t| t.kind == ToolKind::Computer && t.name != REAL_POINTER)
        .cloned()
        .collect();
    let pointer: Vec<_> = tools
        .iter()
        .filter(|t| t.name == REAL_POINTER)
        .cloned()
        .collect();
    let others = tools
        .iter()
        .filter(|t| t.kind != ToolKind::Computer && t.enabled)
        .count();
    let rows = [
        (
            "Computer use",
            "Lets the agent take screenshots and click, type and press keys in your apps. \
             It sees whatever is on the screen.",
            screen,
        ),
        (
            "Your real pointer",
            "Lets the agent move your own mouse when a plain click does not reach a control. \
             It takes over the pointer while it works.",
            pointer,
        ),
    ]
    .into_iter()
    .filter(|(_, _, group)| !group.is_empty())
    .map(|(name, hint, group)| {
        let on = group.iter().all(|t| t.enabled);
        let names: Vec<_> = group.into_iter().map(|t| t.name).collect();
        view! {
            <Row name=name hint=hint>
                <Switch on=on change=move |on| switch(names.clone(), on, reload) />
            </Row>
        }
    })
    .collect_view();
    let none = tools.iter().all(|t| t.kind != ToolKind::Computer);
    view! {
        <div class="welcome-tools">
            {rows}
            {none.then(|| view! {
                <p class="muted small">"This machine has no screen for the agent to use, so there is nothing to switch on."</p>
            })}
        </div>
        <p class="muted small welcome-count">{match others {
            1 => "1 other tool is already on.".to_owned(),
            n => format!("{n} other tools are already on."),
        }}</p>
    }
}

/// One switch over a group: every tool of it, one after another, then the list again,
/// also when a switch failed, so the row shows what the core really has.
fn switch(names: Vec<String>, on: bool, reload: impl Fn() + Send + Sync + 'static) {
    let app = state();
    spawn_local(async move {
        let mut failed = None;
        for name in names {
            if let Err(error) = api::switch(Flag::Tool(name), on).await {
                failed = Some(error);
                break;
            }
        }
        reload();
        if let Some(error) = failed {
            app.fail(error);
        }
    });
}
