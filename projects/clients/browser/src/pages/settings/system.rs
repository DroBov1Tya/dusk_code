//! Settings: every page of the desktop app, each a view over the same core settings.

use crate::{
    api,
    state::state,
    ui::{Row, run, size},
};
use dusk_protocol::client::{Answer, Command};
use leptos::prelude::*;

#[component]
pub fn Updates() -> impl IntoView {
    let app = state();
    let info = RwSignal::new(None::<dusk_protocol::release::UpdateInfo>);
    let check = move || {
        run(api::command(Command::Update { channel: None }), move |a| {
            if let Answer::Update(u) = a {
                info.set(Some(u));
            }
        })
    };
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
pub fn Usage() -> impl IntoView {
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
