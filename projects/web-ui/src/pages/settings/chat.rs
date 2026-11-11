//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    ui::{Section, run},
};
use dusk_protocol::{
    client::{SettingsRequest, Switch as Flag},
    config::{ApprovalPolicy, Settings},
};
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
            let reload = reload.clone();
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
            }
        })}
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
