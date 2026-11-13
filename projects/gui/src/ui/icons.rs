//! The app's own icons, served beside the kit's bundle.

use std::borrow::Cow;

use gpui_kit::*;

/// The moon as a crescent: a disc with a part of it covered by a shadow of its own
/// size, as the system draws the one of Do Not Disturb. Outlined while quiet mode is
/// off, filled while it is on.
pub const MOON: &str = "icons/dusk/moon.svg";
pub const MOON_FILL: &str = "icons/dusk/moon-fill.svg";

const MOON_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"><path d="M8.915 3.546A9 9 0 1 0 20.085 15.954A9 9 0 0 1 8.915 3.546Z" /></svg>"#;

const MOON_FILL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="0.75" stroke-linejoin="round"><path d="M8.915 3.546A9 9 0 1 0 20.085 15.954A9 9 0 0 1 8.915 3.546Z" /></svg>"#;

/// The kit's icons, with ours answered first.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            MOON => return Ok(Some(Cow::Borrowed(MOON_SVG))),
            MOON_FILL => return Ok(Some(Cow::Borrowed(MOON_FILL_SVG))),
            _ => {}
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
