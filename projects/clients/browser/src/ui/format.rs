//! Numbers and times as people read them.

/// Bytes as a short size, as every client says it.
pub use dusk_protocol::thread::short_size as size;

/// A count with thousands marked, such as `12.3k`, as every client says it.
pub use dusk_protocol::thread::short_count as compact;

/// Seconds since the epoch as a local date and time.
pub fn when(seconds: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds as f64 * 1000.0));
    date.to_locale_string("en-GB", &js_sys::Object::new())
        .as_string()
        .unwrap_or_default()
}
