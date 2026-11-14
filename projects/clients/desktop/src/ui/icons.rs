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

/// A speech bubble drawn in dashes: a temporary chat, kept only while the app runs.
pub const TEMPORARY: &str = "icons/dusk/temporary.svg";

const TEMPORARY_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13.5 3.1c-.5 0-1-.1-1.5-.1s-1 .1-1.5.1" /><path d="M19.3 6.8a10.45 10.45 0 0 0-2.1-2.1" /><path d="M20.9 13.5c.1-.5.1-1 .1-1.5s-.1-1-.1-1.5" /><path d="M17.2 19.3a10.45 10.45 0 0 0 2.1-2.1" /><path d="M10.5 20.9c.5.1 1 .1 1.5.1s1-.1 1.5-.1" /><path d="M3.5 17.5 2 22l4.5-1.5" /><path d="M3.1 10.5c0 .5-.1 1-.1 1.5s.1 1 .1 1.5" /><path d="M6.8 4.7a10.45 10.45 0 0 0-2.1 2.1" /></svg>"#;

/// A phone: the iOS Simulator.
pub const PHONE: &str = "icons/dusk/phone.svg";

const PHONE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="20" x="5" y="2" rx="2" ry="2" /><path d="M12 18h.01" /></svg>"#;

/// Lucide's outline, the kit's own style, around the shapes of an icon.
macro_rules! lucide {
    ($shapes:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $shapes,
            "</svg>"
        )
        .as_bytes()
    };
}

/// The iOS Simulator's buttons, which the kit does not ship.
pub const HOME: &str = "icons/dusk/home.svg";
pub const LOCK: &str = "icons/dusk/lock.svg";
pub const POWER: &str = "icons/dusk/power.svg";

const HOME_SVG: &[u8] = lucide!(
    r#"<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" /><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />"#
);
const LOCK_SVG: &[u8] = lucide!(
    r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2" /><path d="M7 11V7a5 5 0 0 1 10 0v4" />"#
);
const POWER_SVG: &[u8] =
    lucide!(r#"<path d="M12 2v10" /><path d="M18.4 6.6a9 9 0 1 1-12.77.04" />"#);

/// The kit's icons, with ours answered first.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            MOON => return Ok(Some(Cow::Borrowed(MOON_SVG))),
            MOON_FILL => return Ok(Some(Cow::Borrowed(MOON_FILL_SVG))),
            TEMPORARY => return Ok(Some(Cow::Borrowed(TEMPORARY_SVG))),
            PHONE => return Ok(Some(Cow::Borrowed(PHONE_SVG))),
            HOME => return Ok(Some(Cow::Borrowed(HOME_SVG))),
            LOCK => return Ok(Some(Cow::Borrowed(LOCK_SVG))),
            POWER => return Ok(Some(Cow::Borrowed(POWER_SVG))),
            _ => {}
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
