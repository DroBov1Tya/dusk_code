//! Which parts this machine is offered and which of them are pinned, and their icons in
//! the header, the rail and beside the message box.
//!
//! A part is pinned by hand from the menu, or by default when it is one of the first
//! `PINNED_FIRST` of its place: one plugin's icons show without a step, a hundred do not
//! crowd the window. A place shows at most `SHOWN` pinned icons; the rest are reached from
//! the menu.

use dusk_protocol::plugin::{Opens, Part, Place};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use super::Key;
use crate::{
    ui::{icons, size, widgets::Focusable},
    workspace::Workspace,
};

/// The most pinned icons a place shows.
pub const SHOWN: usize = 3;
/// How many parts of a place are pinned until the user says otherwise.
const PINNED_FIRST: usize = 2;

/// The parts offered on this platform, in the status's order.
pub fn offered(ws: &Workspace) -> impl Iterator<Item = &Part> {
    let os = std::env::consts::OS;
    ws.status
        .plugin_parts
        .iter()
        .filter(move |part| part.platforms.is_empty() || part.platforms.iter().any(|p| p == os))
}

/// Whether the part is pinned, given its index among the offered parts of its place.
fn pinned_at(part: &Part, index: usize, cx: &App) -> bool {
    let pins = &crate::ui::theme::prefs(cx).plugin_pins;
    pins.get(&Key::of(part).pin())
        .copied()
        .unwrap_or(index < PINNED_FIRST)
}

/// Whether the part is pinned now.
pub fn is_pinned(ws: &Workspace, part: &Part, cx: &App) -> bool {
    let index = offered(ws)
        .filter(|other| other.place == part.place)
        .position(|other| other == part)
        .unwrap_or(usize::MAX);
    pinned_at(part, index, cx)
}

/// The pinned parts a place shows, at most `SHOWN`.
pub fn pinned<'a>(ws: &'a Workspace, place: Place, cx: &App) -> Vec<&'a Part> {
    offered(ws)
        .filter(|part| part.place == place)
        .enumerate()
        .filter(|(index, part)| pinned_at(part, *index, cx))
        .map(|(_, part)| part)
        .take(SHOWN)
        .collect()
}

pub fn toggle_pin(ws: &Workspace, part: &Part, cx: &mut App) {
    let pinned = is_pinned(ws, part, cx);
    let pin = Key::of(part).pin();
    crate::ui::theme::change(cx, |prefs| {
        prefs.plugin_pins.insert(pin, !pinned);
    });
}

/// The part's icon in `color`: its own SVG, which the core checked, or a plain frame (the
/// puzzle is the Plugins button's own).
pub fn icon(part: &Part, side: f32, color: Hsla) -> AnyElement {
    match &part.icon {
        Some(svg_text) => svg()
            .data(svg_text.as_bytes())
            .flex_none()
            .size(px(side))
            .text_color(color)
            .into_any_element(),
        None => Icon::new(IconName::Frame)
            .size(px(side))
            .text_color(color)
            .into_any_element(),
    }
}

/// Whether the part shows now, so its icon reads as pressed.
fn is_shown(ws: &Workspace, key: &Key) -> bool {
    let active = |dock: &super::Dock| !dock.collapsed && dock.tabs.get(dock.active) == Some(key);
    active(&ws.plugins.right)
        || active(&ws.plugins.bottom)
        || matches!(&ws.modal, Some(crate::modal::Modal::Plugin(shown)) if shown == key)
}

/// A square that opens the part, `side` wide with an icon `glyph` wide.
fn square(
    ws: &Workspace,
    part: &Part,
    prefix: &str,
    side: f32,
    glyph: f32,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let key = Key::of(part);
    let shown = is_shown(ws, &key);
    let (hover, selected) = (theme.list_hover, theme.list_active);
    let color = if shown {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let tip = SharedString::from(part.title.clone());
    div()
        .id(ElementId::Name(format!("{prefix}:{}", key.pin()).into()))
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .cursor_pointer()
        .when(shown, |square| square.bg(selected))
        .hover(move |square| square.bg(hover))
        .focusable_row(cx)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(icon(part, glyph, color))
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_plugin_part(key.clone(), cx)))
}

/// The header's pinned icons, then the button of the menu of all parts. Nothing while no
/// plugin adds a part.
pub fn header(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    offered(ws).next()?;
    let icons: Vec<_> = pinned(ws, Place::Header, cx)
        .into_iter()
        .map(|part| square(ws, part, "top-plugin", size::CONTROL, size::ICON, cx))
        .collect();
    let button = Button::new("top-plugins")
        .ghost()
        .icon(Icon::default().path(icons::PUZZLE))
        .tooltip("Plugins")
        .when(ws.plugins.menu, |button| {
            use gpui_kit::component::Selectable;
            button.selected(true)
        })
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_plugins_menu(cx)));
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .children(icons)
            .child(button)
            .into_any_element(),
    )
}

/// The rail's pinned icons, as large as its own places.
pub fn rail(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let parts = pinned(ws, Place::Rail, cx);
    if parts.is_empty() {
        return None;
    }
    let icons: Vec<_> = parts
        .into_iter()
        .map(|part| {
            square(
                ws,
                part,
                "rail-plugin",
                size::CONTROL + 8.,
                size::ICON_LARGE,
                cx,
            )
        })
        .collect();
    Some(
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1p5()
            .children(icons)
            .into_any_element(),
    )
}

/// The icons beside the message box.
pub fn composer(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let parts = pinned(ws, Place::Composer, cx);
    if parts.is_empty() {
        return None;
    }
    let icons: Vec<_> = parts
        .into_iter()
        .map(|part| square(ws, part, "box-plugin", size::CONTROL, size::ICON, cx))
        .collect();
    Some(
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(icons)
            .into_any_element(),
    )
}

/// Where a part opens, said in the menu.
pub fn opens_label(part: &Part) -> &'static str {
    match (part.place, part.opens) {
        (Place::Settings, _) => "Settings",
        (_, Opens::RightPanel) => "Side dock",
        (_, Opens::BottomPanel) => "Bottom dock",
        (_, Opens::Window) => "Window",
    }
}
