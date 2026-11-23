//! The composer's panel, as the desktop app has it: a small tab on the box's upper edge,
//! in sight while the pointer is over the box or the tab, opens a band glued on top of the box with
//! the chat's folder on the left and one ring for the plan's limits on the right; the
//! ring's card lists every window. Open or closed is the page's, never the core's.

use crate::{
    state::state,
    ui::{Icon, IconName, compact},
};
use dusk_protocol::{
    model::{PlanWindow, RateLimits},
    thread::duration_words_secs,
};
use leptos::prelude::*;

#[component]
pub fn Panel() -> impl IntoView {
    let open = RwSignal::new(false);
    view! {
        <div class="composer-panel">
            <div class="composer-tab-slot">
                <button class="composer-tab" class:open=move || open.get()
                    aria-expanded=move || open.get().to_string()
                    aria-label=move || if open.get() { "Close the panel" } else { "Open the panel" }
                    on:click=move |_| open.update(|o| *o = !*o)>
                    <Icon name=IconName::ChevronDown />
                </button>
            </div>
            <Show when=move || open.get()>
                <div class="composer-band">
                    <Folder />
                    <span class="grow"></span>
                    <Limits />
                </div>
            </Show>
        </div>
    }
}

/// The chat's project folder, or the workspace root for a chat without a project.
#[component]
fn Folder() -> impl IntoView {
    let app = state();
    let dir = move || {
        app.project
            .get()
            .and_then(|id| app.project_by_id(&id))
            .map_or_else(|| app.workspace_dir(), |p| p.dir.display().to_string())
    };
    let name = move || {
        let dir = dir();
        let trimmed = dir.trim_end_matches('/');
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_owned()
    };
    view! { <span class="composer-folder muted" title=dir>{name}</span> }
}

/// The fullest of the plan's windows, so a nearly spent week is not hidden behind five
/// fresh hours; none without a plan window.
fn plan_used(limits: &RateLimits) -> Option<u8> {
    limits.plan.iter().map(|w| w.used_percent.min(100)).max()
}

#[component]
fn Limits() -> impl IntoView {
    let app = state();
    let limits = move || {
        app.status
            .with(|s| s.as_ref().and_then(|s| s.limits.clone()))
    };
    move || {
        let limits = limits()?;
        let used = plan_used(&limits)?;
        let tone = match used {
            90.. => "ring small bad",
            75.. => "ring small warn",
            _ => "ring small",
        };
        let minute = [
            ("Requests", limits.requests.clone()),
            ("Tokens", limits.tokens.clone()),
        ];
        Some(view! {
            <div class="limits" tabindex="0" aria-label=format!("Plan usage limits: {used}% used")>
                <div class=tone style:--share=used.to_string()></div>
                <div class="limits-card" role="tooltip">
                    <b>"Plan usage limits"</b>
                    {limits.plan.into_iter().map(window_row).collect_view()}
                    {minute.into_iter().filter_map(|(label, limit)| {
                        let limit = limit.filter(|l| l.limit > 0)?;
                        let mut left = format!("{} of {} left this minute", compact(limit.remaining), compact(limit.limit));
                        if let Some(seconds) = limit.resets_in {
                            left.push_str(&format!(", resets in {}", duration_words_secs(seconds)));
                        }
                        Some(view! { <div class="limits-row"><span class="grow">{label}</span><span class="muted">{left}</span></div> })
                    }).collect_view()}
                </div>
            </div>
        })
    }
}

fn window_row(window: PlanWindow) -> impl IntoView {
    let percent = window.used_percent.min(100);
    let fill = match percent {
        90.. => "tone-bad",
        75.. => "tone-warn",
        _ => "tone-accent",
    };
    view! {
        <div class="limits-window">
            <div class="limits-row">
                <span class="grow">{window.name()}</span>
                {window.resets_in.map(|s| view! { <span class="muted">{format!("Resets in {}", duration_words_secs(s))}</span> })}
                <span class="num">{format!("{percent}%")}</span>
            </div>
            <div class="limits-bar"><span class=fill style:width=format!("{percent}%")></span></div>
        </div>
    }
}
