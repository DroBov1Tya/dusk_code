//! The Subagent models section of the Model page: the strong, default and weak models an
//! agent may hand its subagents, each picked from the connected models or left on the
//! chat's own model.

use crate::{api, state::state, ui::run};
use dusk_protocol::{
    client::SettingsRequest,
    config::{ModelTier, TierState},
    model::ProviderStatus,
};
use leptos::prelude::*;

#[component]
pub fn ModelTiers(#[prop(into)] providers: Signal<Vec<ProviderStatus>>) -> impl IntoView {
    view! {
        <section class="section">
            <h3>"Subagent models"</h3>
            <p class="muted">"An agent picks a level for each subagent it starts. A level left on the chat's model changes nothing."</p>
            {ModelTier::ALL.map(|tier| view! { <TierRow tier providers /> })}
        </section>
    }
}

#[component]
fn TierRow(tier: ModelTier, providers: Signal<Vec<ProviderStatus>>) -> impl IntoView {
    let app = state();
    let shown = move || {
        app.status
            .with(|status| status.as_ref().map(|s| s.model_tiers.get(tier).clone()))
            .unwrap_or_default()
    };
    let (name, hint) = match tier {
        ModelTier::Strong => ("Strong", "Design, review and hard debugging"),
        ModelTier::Default => ("Default", "Most work"),
        ModelTier::Weak => ("Weak", "Mechanical work and read-only scouting"),
    };
    let choose = move |ev| {
        let model = Some(event_target_value(&ev)).filter(|value| !value.is_empty());
        run(
            api::set(SettingsRequest::ModelTier { tier, model }),
            |()| {},
        );
    };
    view! {
        <div class="row">
            <div class="row-text">
                <div class="row-name">{name}</div>
                {move || match shown() {
                    TierState { chosen: Some(model), missing: true } => view! {
                        <div class="row-hint bad">{format!("{model} is not connected, so this level uses the chat's model")}</div>
                    }.into_any(),
                    _ => view! { <div class="row-hint">{hint}</div> }.into_any(),
                }}
            </div>
            <div class="row-control">
                <select aria-label=format!("{name} model") on:change=choose>
                    // Each option says whether it is the choice: a value set on the
                    // select before its options arrive would be lost.
                    {move || {
                        let state = shown();
                        let models = connected(&providers.get());
                        let chosen = state.chosen.clone().unwrap_or_default();
                        // A choice the list lacks (a provider gone or not answering) stays
                        // shown, so the picker never claims another model.
                        let kept = state.chosen.filter(|chosen| !models.contains(chosen)).map(|chosen| {
                            let label = if state.missing { format!("{chosen} (not connected)") } else { chosen.clone() };
                            view! { <option value=chosen selected>{label}</option> }
                        });
                        view! {
                            <option value="" selected=chosen.is_empty()>"The chat's model"</option>
                            {kept}
                            {models.into_iter().map(|model| {
                                let selected = model == chosen;
                                let label = model.clone();
                                view! { <option value=model selected=selected>{label}</option> }
                            }).collect_view()}
                        }
                    }}
                </select>
            </div>
        </div>
    }
}

/// Every model of a provider that is switched on and answered, as `provider/model`.
fn connected(providers: &[ProviderStatus]) -> Vec<String> {
    providers
        .iter()
        .filter(|provider| provider.enabled)
        .filter_map(|provider| Some((provider, provider.models.as_ref().ok()?)))
        .flat_map(|(provider, models)| {
            models
                .iter()
                .map(move |model| format!("{}/{}", provider.name, model.id))
        })
        .collect()
}
