//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Row, Section, Switch, run},
};
use dusk_protocol::{
    client::{SettingsRequest, Switch as Flag},
    config::{ApprovalPolicy, Settings},
    thread::{MAX_AUTO_COMPACT_PERCENT, MIN_AUTO_COMPACT_PERCENT},
};

use super::flip;
use leptos::prelude::*;

#[component]
pub fn ChatPage(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <h2>"Chat"</h2>
        <p class="muted">"How a new chat starts: its mode and what it may do."</p>
        {move || panel.get().map(|p| {
            let (reload, compacted) = (reload.clone(), reload.clone());
            view! {
                <Section title="Mode">
                    <select prop:value=p.mode.clone() on:change=move |ev| {
                        let (name, reload) = (event_target_value(&ev), reload.clone());
                        run(api::set(SettingsRequest::Mode(name)), move |()| reload());
                    }>
                        {p.modes.clone().into_iter().map(|m| view! {
                            <option value=m.name.clone() selected=m.name == p.mode title=m.summary.clone()>{m.name.clone()}</option>
                        }).collect_view()}
                    </select>
                </Section>
                <ApprovalPicker current=p.approvals />
                <AutoCompaction on=p.auto_compact percent=p.auto_compact_percent reload=compacted />
            }
        })}
    }
}

/// The switch of auto-compaction and the share of the context it starts at. The slider
/// is off with the switch, and a warning says what that costs.
#[component]
fn AutoCompaction(
    on: bool,
    percent: u64,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let shown = RwSignal::new(percent);
    let flipped = reload.clone();
    view! {
        <div class="compaction">
            <Row name="Auto-compaction" hint="Summarize older messages when the context fills up">
                <Switch on change=move |on| flip(Flag::AutoCompact, on, flipped.clone()) />
            </Row>
            <Row name="Compact at" hint="How full the context gets before it is summarized">
                <input type="range" min=MIN_AUTO_COMPACT_PERCENT.to_string()
                    max=MAX_AUTO_COMPACT_PERCENT.to_string() step="1" disabled=!on
                    aria-label="Compact at"
                    prop:value=move || shown.get().to_string()
                    on:input=move |ev| if let Ok(n) = event_target_value(&ev).parse() { shown.set(n) }
                    on:change=move |_| {
                        let reload = reload.clone();
                        let request = SettingsRequest::AutoCompactPercent(shown.get_untracked());
                        run(api::number(request), move |_| reload());
                    } />
                <span class="range-value" class:muted=!on>{move || format!("{}%", shown.get())}</span>
            </Row>
            {(!on).then(|| view! { <p class="row-warning">"Chats will stop when the context fills up."</p> })}
        </div>
    }
}

#[component]
pub fn ApprovalPicker(current: ApprovalPolicy) -> impl IntoView {
    let chosen = RwSignal::new(current);
    view! {
        <Section title="What agents may do without asking">
            {[
                (ApprovalPolicy::Ask, "Ask", "Every file outside the project and every command asks you."),
                (ApprovalPolicy::Sandbox, "Sandbox", "Commands run in a sandbox; the rest asks."),
                (ApprovalPolicy::Arbiter, "Auto", "A model decides in your place and explains."),
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
pub fn Safety(
    panel: RwSignal<Option<Settings>>,
    reload: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let _ = reload;
    view! {
        <h2>"Safety"</h2>
        <p class="muted">"What the agents may do without asking."</p>
        {move || panel.get().map(|p| view! { <ApprovalPicker current=p.approvals /> })}
    }
}
