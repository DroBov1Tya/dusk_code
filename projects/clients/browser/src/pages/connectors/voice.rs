//! Connectors: agents, skills, MCP servers, plugins, tools and voice.

use crate::{
    api,
    ui::{Row, Section, confirm, run, sleep},
};
use dusk_protocol::{
    client::{SettingsRequest, VoiceRequest},
    config::{LocalState, SpeechKind, VoiceEndpoint, VoiceModels, VoiceSettings},
};
use leptos::{prelude::*, task::spawn_local};

#[component]
pub fn Voice() -> impl IntoView {
    let models = RwSignal::new(VoiceModels::default());
    let settings = RwSignal::new(VoiceSettings::default());
    let load = move || {
        run(api::voice_models(), move |m| models.set(m));
        run(api::voice_settings(), move |s| settings.set(s));
    };
    load();
    // Installing runs on its own; keep asking while any model installs, until the page goes.
    let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    on_cleanup({
        let alive = std::sync::Arc::clone(&alive);
        move || alive.store(false, std::sync::atomic::Ordering::Relaxed)
    });
    spawn_local(async move {
        while alive.load(std::sync::atomic::Ordering::Relaxed) {
            sleep(2000).await;
            if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let installing = models.with_untracked(|m| {
                m.models
                    .iter()
                    .any(|m| matches!(m.state, LocalState::Installing(_)))
            });
            if installing && let Ok(next) = api::voice_models().await {
                models.set(next);
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
pub fn EndpointForm(
    title: &'static str,
    settings: RwSignal<VoiceSettings>,
    kind: SpeechKind,
) -> impl IntoView {
    let read = move |s: &VoiceSettings| {
        match kind {
            SpeechKind::Recognition => s.stt.clone(),
            SpeechKind::Synthesis => s.tts.clone(),
        }
        .unwrap_or_default()
    };
    let (url, model, voice) = (
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
        RwSignal::new(String::new()),
    );
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
        run(api::set(SettingsRequest::Voice(next.clone())), move |()| {
            settings.set(next)
        });
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
