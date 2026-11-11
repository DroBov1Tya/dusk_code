//! The look of the page. Palettes live in the stylesheet by name; the choice is a
//! convenience of this browser and the only thing the page keeps in its storage.

use wasm_bindgen::JsCast;
use web_sys::HtmlElement;

pub const THEMES: &[&str] = &["Dusk", "Light", "Dark", "Solar Dusk", "Eclipse"];
const KEY: &str = "duskcode-theme";

pub fn slug(name: &str) -> String {
    name.to_ascii_lowercase().replace(' ', "-")
}

pub fn stored() -> String {
    let saved = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten());
    saved
        .filter(|name| THEMES.contains(&name.as_str()))
        .unwrap_or_else(|| THEMES[0].to_owned())
}

/// Shows the theme and remembers it; a browser without storage just forgets.
pub fn apply(name: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(root) = window
        .document()
        .and_then(|document| document.document_element())
        .and_then(|root| root.dyn_into::<HtmlElement>().ok())
    {
        let _ = root.set_attribute("data-theme", &slug(name));
    }
    if let Ok(Some(storage)) = window.local_storage() {
        let _ = storage.set_item(KEY, name);
    }
}
