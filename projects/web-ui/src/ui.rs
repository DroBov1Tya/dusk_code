//! Small pieces every page uses.

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

/// Bytes as a short size.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A count with thousands marked, such as `12.3k`.
pub fn compact(number: u64) -> String {
    match number {
        0..=999 => number.to_string(),
        1_000..=999_999 => format!("{:.1}k", number as f64 / 1e3),
        _ => format!("{:.1}M", number as f64 / 1e6),
    }
}

/// Seconds since the epoch as a local date and time.
pub fn when(seconds: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds as f64 * 1000.0));
    date.to_locale_string("en-GB", &js_sys::Object::new())
        .as_string()
        .unwrap_or_default()
}

/// Runs a call and shows its error as a toast; `then` runs on success.
pub fn run<T: 'static>(
    call: impl std::future::Future<Output = Result<T, String>> + 'static,
    then: impl FnOnce(T) + 'static,
) {
    let app = crate::state::state();
    leptos::task::spawn_local(async move {
        match call.await {
            Ok(value) => then(value),
            Err(error) => app.fail(error),
        }
    });
}

/// Waits `ms` milliseconds.
pub async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
