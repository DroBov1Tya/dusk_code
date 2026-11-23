//! Applies a palette and the chosen fonts to every component. The choice is kept in
//! `gui.json` next to the config: it is how this window looks, not a core setting.

mod code_pad;
mod palettes;

use std::{fs, path::PathBuf, rc::Rc};

use gpui_kit::{
    component::theme::{Theme, ThemeConfig},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use palettes::{PALETTES, Palette};

pub const FONT_SIZES: &[f32] = &[13., 14., 15., 16., 17., 18.];
pub const MONO_SIZES: &[f32] = &[12., 13., 14., 15., 16.];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: String,
    /// `None` is the system font.
    pub font: Option<String>,
    pub font_size: f32,
    pub mono_font: Option<String>,
    pub mono_size: f32,
    /// Chats pinned to the top of the list, by thread ID.
    pub pinned: Vec<String>,
    /// How the side panel lists the chats: under their projects, or all by last activity.
    pub chats_by: crate::columns::sidebar::Order,
    /// The projects folded in the side panel, by ID.
    pub collapsed_projects: Vec<String>,
    /// The right column's folded modules as the user left them; none before the first
    /// choice, so the first start folds the heavy ones.
    pub right_folded: Option<Vec<String>>,
    /// The right column's modules top to bottom as the user dragged them; empty is the
    /// default order.
    pub right_order: Vec<String>,
    /// The side panel's foot, where the chat works: folded until the user opens it.
    pub footer_open: bool,
    /// Scheduled runs up to this moment, in Unix milliseconds, were seen; later ones go
    /// on the "while you were away" card. Zero until the first start sets it.
    pub runs_seen_until: i64,
    /// The version that ran last, to greet an update once; empty before the first start.
    pub seen_version: String,
    /// The header's sky while a turn runs: the glow and the stars.
    pub sky: bool,
    /// Mist at the window's foot after it was left alone for a while.
    pub fog: bool,
    /// Lights over the message box, one for each subagent at work.
    pub fireflies: bool,
    /// Where each part of the window stands; see `ui::layout`.
    pub layout: crate::ui::layout::Layout,
    /// Plugin parts pinned (true) or unpinned (false) by hand, keyed `plugin/part`; a
    /// part not here follows the default of `plugins::place`.
    pub plugin_pins: std::collections::BTreeMap<String, bool>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: palettes::DEFAULT.to_owned(),
            font: None,
            font_size: 16.,
            mono_font: None,
            mono_size: 13.,
            pinned: Vec::new(),
            chats_by: Default::default(),
            collapsed_projects: Vec::new(),
            right_folded: None,
            right_order: Vec::new(),
            footer_open: false,
            runs_seen_until: 0,
            seen_version: String::new(),
            sky: true,
            fog: true,
            fireflies: true,
            layout: Default::default(),
            plugin_pins: Default::default(),
        }
    }
}

struct Stored {
    file: PathBuf,
    prefs: Prefs,
}

impl Global for Stored {}

pub fn init(file: PathBuf, cx: &mut App) {
    // A damaged or missing file falls back to the defaults.
    let mut prefs: Prefs = fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    prefs.layout = prefs.layout.refreshed();
    cx.set_global(Stored { file, prefs });
    apply(cx);
}

pub fn prefs(cx: &App) -> &Prefs {
    &cx.global::<Stored>().prefs
}

/// Changes the preferences, applies them and keeps them.
pub fn change(cx: &mut App, edit: impl FnOnce(&mut Prefs)) {
    edit(&mut cx.global_mut::<Stored>().prefs);
    apply(cx);
    let stored = cx.global::<Stored>();
    let saved = serde_json::to_string_pretty(&stored.prefs)
        .map_err(anyhow::Error::from)
        .and_then(|text| Ok(fs::write(&stored.file, text)?));
    if let Err(error) = saved {
        tracing::warn!("cannot keep the look: {error}");
    }
}

/// The font families installed on this machine, for the settings.
pub fn font_families(cx: &App) -> Vec<String> {
    let mut names: Vec<String> = cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn apply(cx: &mut App) {
    let prefs = prefs(cx).clone();
    let palette = palettes::find(&prefs.theme);
    let config: ThemeConfig = match serde_json::from_value(config(palette, &prefs)) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!("theme {}: {error}", palette.name);
            return;
        }
    };
    let mode = config.mode;
    Theme::update(cx, |theme| theme.apply_config(&Rc::new(config)));
    Theme::change(mode, None, cx);
    // Theme::change reinstalls the library's text defaults, so ours goes after it.
    code_pad::install(cx);
    cx.refresh_windows();
}

/// The one hue the palettes share for names of functions: a calm blue for each mode.
pub fn function_color(dark: bool) -> &'static str {
    if dark { "#8fb4f5" } else { "#1d4ed8" }
}

/// The palette in use.
pub fn palette(cx: &App) -> &'static Palette {
    palettes::find(&prefs(cx).theme)
}

/// How much of the accent lies over selected text in a field or a message, as two hex
/// digits of alpha: enough to see the selection on every surface, and light enough that
/// the words under it stay the page's own.
const SELECTED_TEXT: &str = "40";
/// The same for a terminal's cells, denser: its letters come in sixteen colors, and a
/// selection has to hold them all apart from the screen.
const SELECTED_CELLS: f32 = 0.33;

/// A color of a palette as the window paints it. The palettes are checked by a test, so
/// a value that does not read is clear instead of a crash.
fn hex(value: &str) -> Hsla {
    Rgba::try_from(value).map(Hsla::from).unwrap_or_default()
}

/// The band over the message box and its tab.
pub fn band(cx: &App) -> Hsla {
    hex(palette(cx).band)
}

/// The sixteen colors a terminal program names, and the color of its selected cells.
pub fn terminal(cx: &App) -> ([Hsla; 16], Hsla) {
    let palette = palette(cx);
    (
        palette.terminal.map(hex),
        hex(palette.accent).opacity(SELECTED_CELLS),
    )
}

/// The colors of code in the viewer, from the same palette, so code reads on the theme's
/// own background. The library's default is a light theme's, which left names dark blue
/// on a dark page.
fn highlight(p: &Palette) -> Value {
    let function = function_color(p.dark);
    let color = |value: &str| json!({ "color": value });
    json!({
        "editor.background": p.background,
        "editor.foreground": p.foreground,
        "editor.line_number": p.muted,
        "editor.active_line_number": p.foreground,
        "editor.active_line.background": format!("{}80", p.raised),
        "syntax": {
            "keyword": color(p.accent),
            "title": color(p.accent),
            "tag": color(p.accent),
            "function": color(function),
            "type": color(p.warning),
            "constructor": color(p.warning),
            "attribute": color(p.warning),
            "string": color(p.success),
            "string.escape": color(p.warning),
            "string.regex": color(p.warning),
            "number": color(p.danger),
            "boolean": color(p.danger),
            "constant": color(p.danger),
            "comment": { "color": p.muted, "font_style": "italic" },
            "comment.doc": { "color": p.muted, "font_style": "italic" },
            "property": color(p.foreground),
            "variable": color(p.foreground),
            "variable.special": color(p.secondary),
            "operator": color(p.secondary),
            "punctuation": color(p.secondary),
            "punctuation.bracket": color(p.secondary),
            "punctuation.delimiter": color(p.secondary),
            "embedded": color(p.foreground),
        },
    })
}

/// Spreads the palette's few colors over the component library's many.
fn config(p: &Palette, prefs: &Prefs) -> Value {
    let hairline = format!("{}14", p.foreground);
    let chosen = format!("{}26", p.accent);
    let selected_text = format!("{}{SELECTED_TEXT}", p.accent);
    // A pressed or hovered primary is the accent a little sunk into the page, so the
    // button answers the pointer without a second hue.
    let (primary_hover, primary_active) = (format!("{}e0", p.accent), format!("{}c4", p.accent));
    let (danger_hover, danger_active) = (format!("{}e0", p.danger), format!("{}c4", p.danger));
    // An off switch must still read as a control on every surface, so its track is the
    // text color at low alpha rather than one of the surface steps.
    let switch_off = format!("{}2e", p.foreground);
    let colors: Map<String, Value> = [
        ("background", p.background),
        ("foreground", p.foreground),
        ("border", &hairline),
        ("input.border", &hairline),
        ("ring", p.accent),
        ("caret", p.accent),
        ("selection.background", &selected_text),
        ("muted.background", p.surface),
        ("muted.foreground", p.muted),
        ("accent.background", p.raised),
        ("accent.foreground", p.foreground),
        // A calm primary: the parchment or ink of the ramp, the accent stays rare.
        ("primary.background", p.accent),
        ("primary.foreground", p.on_accent),
        ("primary.hover.background", &primary_hover),
        ("primary.active.background", &primary_active),
        ("secondary.background", p.raised),
        ("secondary.foreground", p.foreground),
        ("secondary.hover.background", p.raised),
        ("secondary.active.background", p.surface),
        ("popover.background", p.surface),
        ("popover.foreground", p.foreground),
        ("list.background", p.panel),
        // A hovered row is a step up from any surface it stands on, a menu's included; the
        // chosen one carries a tint of the accent.
        ("list.hover.background", p.raised),
        ("list.active.background", &chosen),
        ("list.active.border", &hairline),
        ("sidebar.background", p.panel),
        ("sidebar.foreground", p.foreground),
        ("sidebar.border", &hairline),
        ("sidebar.accent.background", p.surface),
        ("sidebar.accent.foreground", p.foreground),
        ("sidebar.primary.background", p.accent),
        ("sidebar.primary.foreground", p.on_accent),
        ("tab.background", "#00000000"),
        ("tab.foreground", p.muted),
        ("tab.active.background", p.raised),
        ("tab.active.foreground", p.foreground),
        ("tab_bar.background", p.panel),
        ("title_bar.background", p.panel),
        ("title_bar.border", &hairline),
        ("scrollbar.background", "#00000000"),
        ("switch.background", &switch_off),
        ("switch.thumb.background", "#ffffff"),
        ("success.background", p.success),
        ("success.foreground", p.on_accent),
        ("warning.background", p.warning),
        ("warning.foreground", p.on_accent),
        ("danger.background", p.danger),
        ("danger.foreground", p.on_accent),
        ("danger.hover.background", &danger_hover),
        ("danger.active.background", &danger_active),
        ("info.background", p.secondary),
        ("info.foreground", p.on_accent),
        ("link.foreground", p.accent),
        ("link.hover.foreground", p.foreground),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), Value::String(value.to_owned())))
    .chain([(
        "scrollbar.thumb.background".to_owned(),
        Value::String(format!("{}40", p.muted)),
    )])
    .collect();
    let mut colors = colors;
    // Absent keys leave the kit's own fallback, the pale tint of `danger`.
    if let Some(solid) = &p.danger_button {
        for (key, value) in [
            ("button.danger.background", solid.background),
            ("button.danger.foreground", solid.foreground),
            ("button.danger.hover.background", solid.hover),
            ("button.danger.active.background", solid.active),
        ] {
            colors.insert(key.to_owned(), Value::String(value.to_owned()));
        }
    }
    let mut config = json!({
        "name": p.name,
        "mode": if p.dark { "dark" } else { "light" },
        "radius": 8,
        "radius.lg": 12,
        "shadow": !p.dark,
        "font.size": prefs.font_size,
        "mono_font.size": prefs.mono_size,
        "colors": colors,
    });
    config["highlight"] = highlight(p);
    if let Some(font) = &prefs.font {
        config["font.family"] = json!(font);
    }
    if let Some(font) = &prefs.mono_font {
        config["mono_font.family"] = json!(font);
    }
    config
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the window's prelude brings a `test` of its own.
    use super::{
        Palette, Prefs, Rgba, SELECTED_TEXT, ThemeConfig, config, palettes, palettes::DangerButton,
    };

    /// WCAG relative luminance.
    fn luminance(color: Rgba) -> f32 {
        let linear = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    #[test]
    fn palette_colors_read_and_selected_text_stands_out_of_every_surface() {
        let alpha = f32::from(u8::from_str_radix(SELECTED_TEXT, 16).unwrap()) / 255.;
        for p in palettes::PALETTES {
            let read = |value: &str| {
                Rgba::try_from(value).unwrap_or_else(|_| panic!("{}: {value}", p.name))
            };
            for value in p.terminal.iter().chain([&p.band]) {
                read(value);
            }
            let accent = read(p.accent);
            for surface in [p.background, p.panel, p.surface] {
                let under = read(surface);
                let mix = |a: f32, b: f32| a * alpha + b * (1. - alpha);
                let selected = Rgba {
                    r: mix(accent.r, under.r),
                    g: mix(accent.g, under.g),
                    b: mix(accent.b, under.b),
                    a: 1.,
                };
                let (a, b) = (luminance(selected), luminance(under));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                // Not text on a fill but a fill beside its page: 1.3 is seen at a glance.
                assert!(ratio >= 1.3, "{} on {surface}: {ratio:.2}", p.name);
            }
        }
    }

    fn palette(danger_button: Option<DangerButton>) -> Palette {
        Palette {
            danger_button,
            ..*palettes::find(palettes::DEFAULT)
        }
    }

    #[test]
    fn a_palette_without_the_solid_danger_keeps_the_kits_tint() {
        let config = config(&palette(None), &Prefs::default());
        let colors = config["colors"].as_object().unwrap();
        assert!(!colors.keys().any(|key| key.starts_with("button.danger")));
        assert!(serde_json::from_value::<ThemeConfig>(config).is_ok());
    }

    #[test]
    fn a_palette_with_it_sets_the_button_colors() {
        let solid = DangerButton {
            background: "#112233",
            foreground: "#ffffff",
            hover: "#223344",
            active: "#334455",
        };
        let config = config(&palette(Some(solid)), &Prefs::default());
        let colors = &config["colors"];
        assert_eq!(colors["button.danger.background"], "#112233");
        assert_eq!(colors["button.danger.foreground"], "#ffffff");
        assert_eq!(colors["button.danger.hover.background"], "#223344");
        assert_eq!(colors["button.danger.active.background"], "#334455");
        assert!(serde_json::from_value::<ThemeConfig>(config).is_ok());
    }
}
