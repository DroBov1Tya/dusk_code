//! The right column of a chat: its plan and tasks, subagents, changed files, background
//! jobs and what the context holds.

use crate::{
    api,
    ui::{compact, run},
};
use dusk_protocol::thread::ContextReport;
use leptos::prelude::*;

#[component]
pub fn ContextTab(id: String) -> impl IntoView {
    let report = RwSignal::new(None::<ContextReport>);
    let load = move || {
        let id = id.clone();
        run(
            async move { api::context_report(Some(id)).await },
            move |r| report.set(Some(r)),
        );
    };
    load();
    view! {
        <div class="context-tab">
            <button class="ghost" on:click=move |_| load()>"Refresh"</button>
            {move || report.get().map(|report| view! {
                <p>{format!("{} of {} tokens", compact(report.total), compact(report.limit))}</p>
                {report.sections.iter().map(|section| view! {
                    <details class="section-row">
                        <summary><span>{section.name.clone()}</span><span class="muted">{compact(section.tokens)}</span></summary>
                        <ul>{section.items.iter().map(|(name, tokens)| view! {
                            <li><span>{name.clone()}</span><span class="muted">{compact(*tokens)}</span></li>
                        }).collect_view()}</ul>
                    </details>
                }).collect_view()}
            })}
        </div>
    }
}
