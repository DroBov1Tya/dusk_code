//! Work a view starts and does not wait on: a call whose error becomes a toast, a timer.

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
