//! Signing in: the token comes once in the address fragment (or is pasted), is traded
//! for a session cookie the page cannot read, and is dropped from the address at once.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestCredentials, RequestInit, Response};

/// The token in the address fragment, taken out of the address bar. The fragment is never
/// sent to a server, and leaving it in the bar would leave it in the history.
pub fn take_fragment_token() -> Option<String> {
    let window = web_sys::window()?;
    let hash = window.location().hash().ok()?;
    let token = hash.trim_start_matches('#').trim();
    if token.is_empty() {
        return None;
    }
    let token = token.to_owned();
    if let Ok(history) = window.history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some("/"));
    }
    Some(token)
}

/// Whether this browser holds a live session.
pub async fn is_signed_in() -> bool {
    matches!(fetch("GET", "/api/session", None).await, Ok(204))
}

/// Trades the token for a session. The error says what to tell the user.
pub async fn sign_in(token: &str) -> Result<(), String> {
    let body = serde_json::json!({ "token": token.trim() }).to_string();
    match fetch("POST", "/api/login", Some(body)).await? {
        204 => Ok(()),
        401 => Err("That token is not right. Copy it again from the terminal.".to_owned()),
        429 => Err("Too many tries. Wait a minute and try again.".to_owned()),
        other => Err(format!("The server answered {other}.")),
    }
}

pub async fn sign_out() {
    let _ = fetch("POST", "/api/logout", None).await;
}

async fn fetch(method: &str, path: &str, body: Option<String>) -> Result<u16, String> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(RequestCredentials::SameOrigin);
    if let Some(body) = body {
        let headers = Headers::new().map_err(js_error)?;
        headers
            .set("content-type", "application/json")
            .map_err(js_error)?;
        init.set_headers(&headers);
        init.set_body(&JsValue::from_str(&body));
    }
    let window = web_sys::window().ok_or("no window")?;
    let promise = window.fetch_with_str_and_init(path, &init);
    let response = JsFuture::from(promise).await.map_err(js_error)?;
    let response: Response = response.dyn_into().map_err(js_error)?;
    Ok(response.status())
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &"message".into())
                .ok()?
                .as_string()
        })
        .unwrap_or_else(|| "the browser refused the request".to_owned())
}
