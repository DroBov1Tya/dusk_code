//! Applies a palette and the chosen fonts to every component. The choice is kept in
//! `gui.json` next to the config: it is how this window looks, not a core setting.

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
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: palettes::DEFAULT.to_owned(),
            font: None,
            font_size: 15.,
            mono_font: None,
            mono_size: 13.,
            pinned: Vec::new(),
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
    let prefs = fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
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
    cx.refresh_windows();
}

/// Spreads the palette's few colors over the component library's many.
fn config(p: &Palette, prefs: &Prefs) -> Value {
    let hairline = format!("{}14", p.foreground);
    let soft = format!("{}1f", p.accent);
    let colors: Map<String, Value> = [
        ("background", p.background),
        ("foreground", p.foreground),
        ("border", &hairline),
        ("input.border", &hairline),
        ("ring", p.accent),
        ("caret", p.accent),
        ("selection.background", &soft),
        ("muted.background", p.surface),
        ("muted.foreground", p.muted),
        ("accent.background", p.raised),
        ("accent.foreground", p.foreground),
        // A calm primary: the parchment or ink of the ramp, the accent stays rare.
        ("primary.background", p.accent),
        ("primary.foreground", p.on_accent),
        ("primary.hover.background", p.accent),
        ("primary.active.background", p.accent),
        ("secondary.background", p.raised),
        ("secondary.foreground", p.foreground),
        ("secondary.hover.background", p.raised),
        ("secondary.active.background", p.surface),
        ("popover.background", p.surface),
        ("popover.foreground", p.foreground),
        ("list.background", p.panel),
        ("list.hover.background", p.surface),
        ("list.active.background", p.raised),
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
        ("switch.background", p.raised),
        ("success.background", p.success),
        ("success.foreground", p.on_accent),
        ("warning.background", p.warning),
        ("warning.foreground", p.on_accent),
        ("danger.background", p.danger),
        ("danger.foreground", p.on_accent),
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
    if let Some(font) = &prefs.font {
        config["font.family"] = json!(font);
    }
    if let Some(font) = &prefs.mono_font {
        config["mono_font.family"] = json!(font);
    }
    config
}
