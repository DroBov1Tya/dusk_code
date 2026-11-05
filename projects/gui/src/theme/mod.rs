//! Applies a palette to every component, and keeps the choice in a file of its own
//! next to the config: it is how this window looks, not a setting of the core.

mod palettes;

use std::{fs, path::PathBuf, rc::Rc};

use gpui_kit::{
    component::theme::{Theme, ThemeConfig},
    *,
};
use serde_json::json;

pub use palettes::{PALETTES, Palette};

/// Where the chosen theme's name is kept.
struct ThemeFile(PathBuf);

impl Global for ThemeFile {}

pub fn init(file: PathBuf, cx: &mut App) {
    let name = fs::read_to_string(&file).unwrap_or_default();
    cx.set_global(ThemeFile(file));
    let name = name.trim();
    apply(if name.is_empty() { palettes::DEFAULT } else { name }, cx);
}

pub fn current(cx: &App) -> &'static Palette {
    palettes::find(Theme::global(cx).theme_name())
}

/// Switches to the palette named `name` and remembers it.
pub fn choose(name: &str, cx: &mut App) {
    apply(name, cx);
    let file = &cx.global::<ThemeFile>().0;
    if let Err(error) = fs::write(file, name) {
        tracing::warn!("cannot keep the theme: {error}");
    }
}

fn apply(name: &str, cx: &mut App) {
    let palette = palettes::find(name);
    let config: ThemeConfig = match serde_json::from_value(config(palette)) {
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
fn config(p: &Palette) -> serde_json::Value {
    let hover = format!("{}cc", p.accent);
    let soft = format!("{}26", p.accent);
    let colors: serde_json::Map<String, serde_json::Value> = [
        ("background", p.background.to_string()),
        ("foreground", p.foreground.to_string()),
        ("border", p.border.to_string()),
        ("input.border", p.border.to_string()),
        ("ring", p.accent.to_string()),
        ("caret", p.accent.to_string()),
        ("selection.background", soft.to_string()),
        ("muted.background", p.surface.to_string()),
        ("muted.foreground", p.muted.to_string()),
        ("accent.background", p.surface.to_string()),
        ("accent.foreground", p.foreground.to_string()),
        ("primary.background", p.accent.to_string()),
        ("primary.foreground", p.on_accent.to_string()),
        ("primary.hover.background", hover.to_string()),
        ("primary.active.background", p.accent.to_string()),
        ("secondary.background", p.surface.to_string()),
        ("secondary.foreground", p.foreground.to_string()),
        ("secondary.hover.background", p.border.to_string()),
        ("secondary.active.background", p.border.to_string()),
        ("popover.background", p.panel.to_string()),
        ("popover.foreground", p.foreground.to_string()),
        ("list.background", p.panel.to_string()),
        ("list.hover.background", p.surface.to_string()),
        ("list.active.background", soft.to_string()),
        ("list.active.border", p.accent.to_string()),
        ("sidebar.background", p.panel.to_string()),
        ("sidebar.foreground", p.foreground.to_string()),
        ("sidebar.border", p.border.to_string()),
        ("sidebar.accent.background", p.surface.to_string()),
        ("sidebar.accent.foreground", p.foreground.to_string()),
        ("sidebar.primary.background", p.accent.to_string()),
        ("sidebar.primary.foreground", p.on_accent.to_string()),
        ("tab.background", "#00000000".to_string()),
        ("tab.foreground", p.muted.to_string()),
        ("tab.active.background", p.surface.to_string()),
        ("tab.active.foreground", p.foreground.to_string()),
        ("tab_bar.background", p.panel.to_string()),
        ("title_bar.background", p.panel.to_string()),
        ("title_bar.border", p.border.to_string()),
        ("scrollbar.background", "#00000000".to_string()),
        ("scrollbar.thumb.background", format!("{}99", p.muted).to_string()),
        ("switch.background", p.border.to_string()),
        ("success.background", p.success.to_string()),
        ("success.foreground", p.on_accent.to_string()),
        ("warning.background", p.warning.to_string()),
        ("warning.foreground", p.on_accent.to_string()),
        ("danger.background", p.danger.to_string()),
        ("danger.foreground", p.on_accent.to_string()),
        ("info.background", p.accent.to_string()),
        ("info.foreground", p.on_accent.to_string()),
        ("link.foreground", p.accent.to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), serde_json::Value::String(value)))
    .collect();
    json!({
        "name": p.name,
        "mode": if p.dark { "dark" } else { "light" },
        "radius": 8,
        "radius.lg": 12,
        "shadow": false,
        "colors": colors,
    })
}
