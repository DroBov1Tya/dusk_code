//! Controls every page draws: switches, titled sections, rows, a yes-or-no question.

use leptos::prelude::*;

/// A toggle that calls `change` with the new state.
#[component]
pub fn Switch(
    #[prop(into)] on: Signal<bool>,
    change: impl Fn(bool) + 'static,
    #[prop(optional, into)] label: String,
) -> impl IntoView {
    view! {
        <label class="switch">
            <input
                type="checkbox"
                prop:checked=move || on.get()
                on:change=move |ev| change(event_target_checked(&ev))
            />
            <span class="track"></span>
            <span class="label">{label}</span>
        </label>
    }
}

/// A titled block of a settings page.
#[component]
pub fn Section(#[prop(into)] title: String, children: Children) -> impl IntoView {
    view! {
        <section class="section">
            <h3>{title}</h3>
            {children()}
        </section>
    }
}

/// A row with a name, a hint and a control.
#[component]
pub fn Row(
    #[prop(into)] name: String,
    #[prop(optional, into)] hint: String,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="row">
            <div class="row-text">
                <div class="row-name">{name}</div>
                <div class="row-hint">{hint}</div>
            </div>
            <div class="row-control">{children()}</div>
        </div>
    }
}

/// A native yes/no question; the page has nothing else to ask it with.
pub fn confirm(question: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.confirm_with_message(question).ok())
        .unwrap_or(false)
}
