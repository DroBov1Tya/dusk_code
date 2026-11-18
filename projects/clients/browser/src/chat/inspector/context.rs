//! What the context holds: the total against the limit and each part, which unfolds to
//! what it is made of. The Overview draws it as one of its sections.

use crate::ui::compact;
use dusk_protocol::thread::ContextReport;
use leptos::prelude::*;

pub fn report_view(report: ContextReport) -> impl IntoView {
    view! {
        <p>{format!("{} of {} tokens", compact(report.total), compact(report.limit))}</p>
        {report.sections.into_iter().map(|section| view! {
            <details class="section-row">
                <summary><span>{section.name}</span><span class="muted">{compact(section.tokens)}</span></summary>
                <ul>{section.items.into_iter().map(|(name, tokens)| view! {
                    <li><span>{name}</span><span class="muted">{compact(tokens)}</span></li>
                }).collect_view()}</ul>
            </details>
        }).collect_view()}
    }
}
