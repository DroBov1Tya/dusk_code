//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use crate::{
    api,
    ui::{Row, Section, Switch, run},
};
use dusk_protocol::{
    client::Switch as Flag,
    config::{Settings, ToolKind},
};
use leptos::prelude::*;

/// Tools by kind. Computer use is the desktop app's, and has no switch here.
#[component]
pub fn Tools(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
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
