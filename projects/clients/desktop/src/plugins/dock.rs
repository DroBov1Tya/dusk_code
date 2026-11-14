//! The docks parts open in: one at the right of the work, one under it. Each holds every
//! open part of its side as a tab, so many plugins never add columns; the strip of tabs
//! scrolls sideways, and one control folds the dock away and back.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use super::{
    Dock, Key, place,
    view::{self, Room},
};
use crate::{
    ui::{Type, size},
    workspace::Workspace,
};

/// The right dock's width, as the simulator's panel.
pub const WIDTH: f32 = 400.;
/// The right dock folded: a strip of its tabs' icons.
const FOLDED: f32 = 44.;
/// The bottom dock's height at first, the least a drag of its edge leaves, and the most
/// as a share of the window's height; and its strip of tabs.
pub const HEIGHT: f32 = 300.;
const MIN_HEIGHT: f32 = 160.;
const MAX_SHARE: f32 = 0.7;
const TABS: f32 = 36.;
const PADDING: f32 = 16.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Right,
    Bottom,
}

impl Side {
    fn dock(self, ws: &mut Workspace) -> &mut Dock {
        match self {
            Side::Right => &mut ws.plugins.right,
            Side::Bottom => &mut ws.plugins.bottom,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Bottom => "bottom",
        }
    }
}

/// The dock at the right edge, while a part is open in it.
pub fn right(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let dock = &ws.plugins.right;
    let key = dock.tabs.get(dock.active)?.clone();
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    let frame = div()
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .bg(panel)
        .border_l_1()
        .border_color(border);
    if dock.collapsed {
        return Some(
            frame
                .w(px(FOLDED))
                .items_center()
                .gap_1()
                .py_2()
                .child(fold_button(Side::Right, true, cx))
                .children(dock.tabs.iter().enumerate().filter_map(|(index, key)| {
                    let part = super::part(ws, key)?;
                    Some(icon_tab(Side::Right, index, part, cx))
                })),
        );
    }
    let head = div()
        .h(px(size::BAR))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().flex_1().min_w_0().child(strip(ws, Side::Right, cx)))
        .child(fold_button(Side::Right, false, cx))
        .child(div().w_2());
    let room = Room {
        width: WIDTH - 2. * PADDING,
        height: f32::INFINITY,
    };
    Some(
        frame.w(px(WIDTH)).child(head).child(
            div()
                .id("plugin-dock-right")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(PADDING))
                .child(view::render(ws, &key, room, cx)),
        ),
    )
}

/// The dock under the work, while a part is open in it.
pub fn bottom(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Option<Div> {
    let dock = &ws.plugins.bottom;
    let key = dock.tabs.get(dock.active)?.clone();
    let theme = cx.theme();
    let (border, surface) = (theme.border, theme.background);
    let bar = div()
        .h(px(TABS))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .child(div().flex_1().min_w_0().child(strip(ws, Side::Bottom, cx)))
        .child(fold_button(Side::Bottom, dock.collapsed, cx))
        .child(div().w_2());
    let frame = div().flex_none().flex().flex_col().bg(surface);
    if dock.collapsed {
        return Some(frame.border_t_1().border_color(border).child(bar));
    }
    let height = bottom_height(ws.plugins.bottom_height, window);
    let room = Room {
        width: (ws.chat_width(window) - 2. * PADDING).max(1.),
        height: height - TABS - PADDING,
    };
    // The edge is the handle, as the terminal's: drag it for more room or less.
    let edge = div()
        .id("plugin-dock-edge")
        .h(px(5.))
        .flex_none()
        .border_t_1()
        .border_color(border)
        .cursor_row_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &MouseDownEvent, _, cx| {
                ws.plugins.bottom_drag = Some((event.position.y, height));
                cx.notify();
            }),
        );
    Some(
        frame.h(px(height)).child(edge).child(bar).child(
            div()
                .id("plugin-dock-bottom")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(PADDING))
                .pt_1()
                .pb(px(PADDING))
                .child(view::render(ws, &key, room, cx)),
        ),
    )
}

/// The dock's tabs in a strip that scrolls sideways.
fn strip(ws: &Workspace, side: Side, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let dock = match side {
        Side::Right => &ws.plugins.right,
        Side::Bottom => &ws.plugins.bottom,
    };
    let mut strip = div()
        .id(ElementId::Name(
            format!("plugin-tabs-{}", side.name()).into(),
        ))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .w_full()
        .px_2()
        .overflow_x_scroll()
        .track_scroll(&dock.strip);
    for (index, key) in dock.tabs.iter().enumerate() {
        strip = strip.child(tab(ws, side, index, key, index == dock.active, cx));
    }
    strip
}

/// A tab: the part's icon and title, and its close button.
fn tab(
    ws: &Workspace,
    side: Side,
    index: usize,
    key: &Key,
    chosen: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent, foreground) = (theme.muted_foreground, theme.primary, theme.foreground);
    let color = if chosen { foreground } else { muted };
    let id =
        |kind: &str| ElementId::Name(format!("plugin-{kind}-{}:{}", side.name(), key.pin()).into());
    let closed = key.clone();
    div()
        .id(id("tab"))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .h(px(28.))
        .pl_2()
        .pr_1()
        .rounded_md()
        .small()
        .cursor_pointer()
        .text_color(color)
        .when(chosen, |tab| tab.bg(accent.opacity(0.14)))
        .children(super::part(ws, key).map(|part| place::icon(part, 14., color)))
        .child(
            div()
                .max_w(px(160.))
                .truncate()
                .child(super::title(ws, key)),
        )
        .child(
            Button::new(id("close"))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Close")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    ws.close_plugin_part(closed.clone(), cx);
                })),
        )
        .on_click(cx.listener(move |ws, _, _, cx| {
            side.dock(ws).active = index;
            cx.notify();
        }))
}

/// A folded right dock's tab: its icon; a click unfolds the dock on it.
fn icon_tab(
    side: Side,
    index: usize,
    part: &dusk_protocol::plugin::Part,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (hover, muted) = (theme.list_hover, theme.muted_foreground);
    let tip = SharedString::from(part.title.clone());
    div()
        .id(ElementId::Name(
            format!("plugin-folded-{}:{}", side.name(), Key::of(part).pin()).into(),
        ))
        .size(px(size::CONTROL))
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .cursor_pointer()
        .hover(move |tab| tab.bg(hover))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(place::icon(part, size::ICON, muted))
        .on_click(cx.listener(move |ws, _, _, cx| {
            let dock = side.dock(ws);
            dock.active = index;
            dock.collapsed = false;
            cx.notify();
        }))
}

/// The one control that folds the dock away, or brings it back.
fn fold_button(side: Side, folded: bool, cx: &mut Context<Workspace>) -> Button {
    let (icon, tip) = match (side, folded) {
        (Side::Right, false) => (IconName::ChevronRight, "Fold the dock away"),
        (Side::Right, true) => (IconName::ChevronLeft, "Show the dock"),
        (Side::Bottom, false) => (IconName::ChevronDown, "Fold the dock away"),
        (Side::Bottom, true) => (IconName::ChevronUp, "Show the dock"),
    };
    Button::new(ElementId::Name(
        format!("plugin-fold-{}", side.name()).into(),
    ))
    .ghost()
    .icon(icon)
    .tooltip(tip)
    .on_click(cx.listener(move |ws, _, _, cx| {
        let dock = side.dock(ws);
        dock.collapsed = !dock.collapsed;
        cx.notify();
    }))
}

/// The bottom dock's height kept within its bounds for this window, which may have shrunk
/// since it was set.
fn bottom_height(height: f32, window: &Window) -> f32 {
    let most = window.viewport_size().height.as_f32() * MAX_SHARE;
    height.min(most).max(MIN_HEIGHT)
}

impl Workspace {
    /// The pointer moved while the bottom dock's edge is held: the dock follows it.
    pub fn drag_plugin_dock(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some((from, height)) = self.plugins.bottom_drag else {
            return;
        };
        // Released outside the window, the button is up without a word: stop here.
        if event.pressed_button != Some(MouseButton::Left) {
            self.plugins.bottom_drag = None;
        } else {
            let moved = (from - event.position.y).as_f32();
            self.plugins.bottom_height = bottom_height(height + moved, window);
        }
        cx.notify();
    }
}
