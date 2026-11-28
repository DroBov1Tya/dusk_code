//! The Access page: one choice per approval mode, what the agent does in it without
//! asking and what it still asks for. A pick is saved at once as the default for new
//! chats, the setting Safety shows; nothing is picked for the user, so Next keeps it.

use crate::{api, ui::run};
use dusk_protocol::{
    client::Switch as Flag,
    config::{ApprovalPolicy, Settings},
};
use leptos::prelude::*;

/// The modes from the least the agent does alone to the most.
const MODES: [(ApprovalPolicy, &str, &str); 4] = [
    (
        ApprovalPolicy::Ask,
        "Ask",
        "Edits files in your project on its own. Asks before running commands and before \
         anything outside the project.",
    ),
    (
        ApprovalPolicy::Sandbox,
        "Sandbox",
        "Runs commands without asking, locked so they can write only in your project. Asks \
         before anything outside it.",
    ),
    (
        ApprovalPolicy::Arbiter,
        "Auto",
        "A second model allows or refuses each request for you and says why. When it \
         cannot decide, it asks you.",
    ),
    (
        ApprovalPolicy::FullAccess,
        "Full access",
        "Runs commands and edits files without asking; only settings changes and steps on \
         your own branches still ask. A wrong command runs before you can stop it, so use it \
         where the work can be undone.",
    ),
];

#[component]
pub fn Access() -> impl IntoView {
    let panel = RwSignal::new(None::<Settings>);
    run(api::settings_panel(), move |p| panel.set(Some(p)));
    view! {
        <div class="welcome-page">
            <h2>"How much may the agent do on its own?"</h2>
            <p class="muted">"This is the default for new chats."</p>
            {move || panel.get().map(|p| choices(p.approvals, p.sandbox))}
            <p class="muted small">"Change it any time in Settings, under Safety, or for one chat with the access menu by the message box."</p>
        </div>
    }
}

fn choices(current: ApprovalPolicy, sandbox: bool) -> impl IntoView {
    let chosen = RwSignal::new(current);
    let rows = MODES.map(|(policy, name, about)| {
        let usable = policy != ApprovalPolicy::Sandbox || sandbox;
        let about = match usable {
            true => about,
            false => "Not available here: this system cannot lock commands to the project.",
        };
        view! {
            <label class="option" class:disabled=!usable>
                <input type="radio" name="welcome-access" disabled=!usable
                    prop:checked=move || chosen.get() == policy
                    on:change=move |_| run(api::switch(Flag::Approvals(policy), true), move |()| chosen.set(policy)) />
                <span class="welcome-access-text">
                    <span class="welcome-access-name">
                        <b>{name}</b>
                        {(policy == ApprovalPolicy::FullAccess).then(|| view! {
                            <span class="welcome-badge">"Recommended if you don't want to be asked"</span>
                        })}
                    </span>
                    <span class="muted small">{about}</span>
                </span>
            </label>
        }
    });
    view! { <div class="welcome-access">{rows.into_iter().collect_view()}</div> }
}
