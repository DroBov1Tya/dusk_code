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

/// Two arrows chasing each other round: ask again.
pub const REFRESH: &str = "icons/dusk/refresh.svg";

const REFRESH_SVG: &[u8] = lucide!(
    r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /><path d="M8 16H3v5" />"#
);

/// Plugins: the one button their parts wait behind, and the pin that keeps a part's icon
/// in its place.
pub const PUZZLE: &str = "icons/dusk/puzzle.svg";
pub const PIN: &str = "icons/dusk/pin.svg";

const PUZZLE_SVG: &[u8] = lucide!(
    r#"<path d="M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z" />"#
);
const PIN_SVG: &[u8] = lucide!(
    r#"<path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />"#
);

pub const IMAGE: &str = "icons/dusk/image.svg";
pub const VIDEO: &str = "icons/dusk/video.svg";

const IMAGE_SVG: &[u8] = lucide!(
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
);
const VIDEO_SVG: &[u8] =
    lucide!(r#"<rect x="2" y="6" width="14" height="12" rx="2"/><path d="m16 10 5-3v10l-5-3"/>"#);

pub const HOME: &str = "icons/dusk/home.svg";
pub const LOCK: &str = "icons/dusk/lock.svg";
pub const POWER: &str = "icons/dusk/power.svg";

const HOME_SVG: &[u8] = lucide!(
    r#"<path d="m3 10 9-7 9 7v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M9 21v-7h6v7"/>"#
);
const LOCK_SVG: &[u8] = lucide!(
    r#"<rect x="4" y="11" width="16" height="11" rx="2"/><path d="M8 11V6a4 4 0 1 1 8 0v5"/>"#
);
const POWER_SVG: &[u8] = lucide!(r#"<path d="M12 2v10M6 5a9 9 0 1 0 12 0"/>"#);

/// The kit's icons, with ours answered first.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match path {
            MOON => return Ok(Some(Cow::Borrowed(MOON_SVG))),
            MOON_FILL => return Ok(Some(Cow::Borrowed(MOON_FILL_SVG))),
            TEMPORARY => return Ok(Some(Cow::Borrowed(TEMPORARY_SVG))),
            REFRESH => return Ok(Some(Cow::Borrowed(REFRESH_SVG))),
            IMAGE => return Ok(Some(Cow::Borrowed(IMAGE_SVG))),
            VIDEO => return Ok(Some(Cow::Borrowed(VIDEO_SVG))),
            HOME => return Ok(Some(Cow::Borrowed(HOME_SVG))),
            LOCK => return Ok(Some(Cow::Borrowed(LOCK_SVG))),
            POWER => return Ok(Some(Cow::Borrowed(POWER_SVG))),
            PUZZLE => return Ok(Some(Cow::Borrowed(PUZZLE_SVG))),
            PIN => return Ok(Some(Cow::Borrowed(PIN_SVG))),
            _ => {}
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}
