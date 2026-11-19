//! A letter avatar for a connector: the first letter of its name on a tile whose color
//! the name picks, so the same connector looks the same on every run and machine.

use crate::ui::Type;
use gpui_kit::{component::ActiveTheme, *};

/// How many colors the name chooses from.
const SHADES: usize = 8;
/// How much of the picked color is mixed into the surface. The label stays the plain
/// text color, which keeps its contrast on every theme.
const TINT: f32 = 0.25;

/// The first letter or digit of `name` in upper case; `?` when it has none.
pub fn letter(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// FNV-1a of `name`, which unlike `RandomState` gives the same answer on every run and
/// machine.
pub fn hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Which of the `SHADES` colors `name` takes.
pub fn shade(name: &str) -> usize {
    (hash(name) % SHADES as u64) as usize
}

/// The color of a shade: four theme colors, then their opposites on the color wheel.
pub fn tone(shade: usize, cx: &App) -> Hsla {
    let theme = cx.theme();
    let base = [theme.primary, theme.success, theme.warning, theme.danger];
    let mut color = base[shade % base.len()];
    if shade % SHADES >= base.len() {
        color.h = (color.h + 0.5) % 1.;
    }
    color
}

/// The tile for `name`, `size` px square. `bundled_icon` is the slot for a picture the
/// signed catalog ships for the connector; when present it is drawn instead of the letter.
pub fn avatar(name: &str, size: f32, bundled_icon: Option<std::path::PathBuf>, cx: &App) -> Div {
    let tile = div().size(px(size)).flex_none().rounded_md();
    if let Some(path) = bundled_icon {
        return tile.child(img(path).size_full().rounded_md());
    }
    let theme = cx.theme();
    let color = tone(shade(name), cx);
    tile.bg(theme.muted.blend(color.opacity(TINT)))
        .flex()
        .items_center()
        .justify_center()
        .small()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(letter(name))
}

#[cfg(test)]
mod tests {
    use super::{SHADES, letter, shade};

    #[test]
    fn letter_skips_symbols_and_never_slices_bytes() {
        assert_eq!(letter("github"), "G");
        assert_eq!(letter("_-1password"), "1");
        assert_eq!(letter("  "), "?");
        assert_eq!(letter(""), "?");
        assert_eq!(letter("élan"), "\u{c9}");
        assert_eq!(letter("\u{1f680}\u{43f}\u{440}"), "\u{41f}");
    }

    #[test]
    fn shade_is_stable_and_in_range() {
        assert_eq!(shade("github"), shade("github"));
        // Pinned, so a change to the hash (which would recolor every connector) is seen.
        assert_eq!(shade(""), 5);
        assert!((0..50).all(|n| shade(&format!("server-{n}")) < SHADES));
    }
}
