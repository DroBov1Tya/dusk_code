//! Numbers and times as people read them.

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
