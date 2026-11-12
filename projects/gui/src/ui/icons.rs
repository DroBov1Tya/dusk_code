//! The app's own icons, served beside the kit's bundle.

use std::borrow::Cow;

use gpui_kit::*;

/// The moon filled in: quiet mode is on.
pub const MOON_FILL: &str = "icons/dusk/moon-fill.svg";

const MOON_FILL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401" /></svg>"#;

/// The kit's icons, with ours answered first.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == MOON_FILL {
            return Ok(Some(Cow::Borrowed(MOON_FILL_SVG)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
