//! A part's view drawn with the app's own widgets and type scale. Text is text, never
//! markup; each widget that can be used sends its event back under its id.

use dusk_core::app::PhoneDependency;
use dusk_protocol::plugin::{ButtonIcon, RowLayout, Size, Tone, TouchPhase, UiEvent, Widget};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::{Button, ButtonCustomVariant, ButtonVariants},
        input::Input,
    },
    prelude::FluentBuilder,
    *,
};

use super::{Key, Slot, lists};
use crate::{
    ui::{Type, icons, phone, size},
    workspace::Workspace,
};

/// The widest a phone is drawn: a panel wider than this leaves margins around it.
const PHONE: f32 = 440.;

/// The room a view has: its width, and its height where it does not scroll (infinite
/// where it does).
#[derive(Clone, Copy)]
pub struct Room {
    pub width: f32,
    pub height: f32,
}

/// The part's view, or a wait until the plugin sends one.
pub fn render(ws: &Workspace, key: &Key, room: Room, cx: &mut Context<Workspace>) -> AnyElement {
    match ws.plugins.views.get(key) {
        Some(root) => {
            let content = widget(ws, key, root, room, cx);
            let Some(error) = ws.plugins.dependency_errors.get(key) else {
                return content;
            };
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(text(error, Tone::Danger, Size::Small, cx))
                .child(content)
                .into_any_element()
        }
        None => crate::ui::loading("Opening", cx).into_any_element(),
    }
}

/// Every widget of a view, depth first.
pub fn each(widget: &Widget, visit: &mut impl FnMut(&Widget)) {
    visit(widget);
    if let Widget::Column { children } | Widget::Row { children, .. } = widget {
        for child in children {
            each(child, visit);
        }
    }
}

pub(super) fn has_dependency_setup(root: &Widget) -> bool {
    let mut found = false;
    each(root, &mut |widget| {
        found |= matches!(
            widget,
            Widget::Text {
                dependency: true,
                ..
            }
        );
    });
    found
}

/// An element id unique to the widget `id` of `key`.
pub(super) fn eid(key: &Key, kind: &str, id: &str) -> ElementId {
    ElementId::Name(format!("plugin:{}:{}:{kind}:{id}", key.plugin, key.part).into())
}

fn widget(
    ws: &Workspace,
    key: &Key,
    widget: &Widget,
    room: Room,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    match widget {
        Widget::Column { children } => div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .children(
                children
                    .iter()
                    .map(|child| self::widget(ws, key, child, room, cx)),
            )
            .into_any_element(),
        Widget::Row { children, layout } => row(ws, key, children, *layout, room, cx),
        Widget::Text {
            text,
            tone,
            size,
            dependency,
        } => {
            let message = self::text(text, *tone, *size, cx);
            match (
                dependency,
                PhoneDependency::for_panel(&key.plugin, &key.part),
            ) {
                (true, Some(dependency)) => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(message)
                    .child(dependency_action(ws, key, dependency, cx))
                    .into_any_element(),
                _ => message,
            }
        }
        Widget::Button {
            id,
            label,
            icon,
            primary,
            disabled,
        } => {
            let (key, id) = (key.clone(), id.clone());
            let button = Button::new(eid(&key, "button", &id));
            let button = match icon.and_then(glyph) {
                // An icon is a quiet tool with its label as a tooltip; the one that switches
                // a device off warns under the pointer.
                Some(glyph) => button
                    .small()
                    .icon(Icon::default().path(glyph))
                    .tooltip(label.clone())
                    .accessibility_label(label.clone())
                    .map(|button| match icon {
                        Some(ButtonIcon::Power) => button.custom(warning_tool(cx)),
                        _ => button.ghost(),
                    }),
                None => button.label(label.clone()),
            };
            // A button keeps its own width; a column would stretch it across the panel.
            div()
                .flex()
                .child(
                    button
                        .when(*primary, |button| button.primary())
                        .disabled(*disabled)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.plugin_event(&key, &id, UiEvent::Click, cx);
                        })),
                )
                .into_any_element()
        }
        Widget::Input { id, .. } => match ws.plugins.fields.get(&(key.clone(), id.clone())) {
            Some(field) => div()
                .w_full()
                .child(Input::new(&field.state))
                .into_any_element(),
            // Its box comes with the next draw.
            None => div().h(px(size::CONTROL)).into_any_element(),
        },
        Widget::List {
            id,
            items,
            selected,
            menu: false,
        } => lists::list(key, id, items, selected.as_deref(), cx),
        Widget::List {
            id,
            items,
            selected,
            menu: true,
        } => lists::dropdown(ws, key, id, items, selected.as_deref(), false, cx),
        Widget::Screen {
            id,
            width,
            height,
            phone,
        } => screen(
            ws,
            key,
            id,
            (*width as f32, *height as f32),
            *phone,
            room,
            cx,
        ),
        Widget::Progress { text } => {
            let text = if text.is_empty() { "Working" } else { text };
            crate::ui::loading(text, cx).into_any_element()
        }
        Widget::Divider => div()
            .w_full()
            .h(px(1.))
            .bg(cx.theme().border)
            .into_any_element(),
    }
}

/// A row. A toolbar gives its menu or text the room left, drawn compact, and its buttons
/// their own width; a centered row of icons alone is one small bar of tools. A power
/// button stands apart from the buttons before it, so it is not pressed for its neighbor.
fn row(
    ws: &Workspace,
    key: &Key,
    children: &[Widget],
    layout: RowLayout,
    room: Room,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (border, bar) = (theme.border, theme.secondary);
    let tools = layout == RowLayout::Center && !children.is_empty() && children.iter().all(is_tool);
    let mut cells = Vec::with_capacity(children.len() + 1);
    for (index, child) in children.iter().enumerate() {
        if index > 0
            && matches!(
                child,
                Widget::Button {
                    icon: Some(ButtonIcon::Power),
                    ..
                }
            )
        {
            cells.push(rule(border));
        }
        let cell = match (layout, child) {
            (
                RowLayout::Toolbar,
                Widget::List {
                    id,
                    items,
                    selected,
                    menu: true,
                },
            ) => lists::dropdown(ws, key, id, items, selected.as_deref(), true, cx),
            _ => self::widget(ws, key, child, room, cx),
        };
        let cell = match layout {
            RowLayout::Toolbar => div()
                .when(
                    matches!(child, Widget::List { menu: true, .. } | Widget::Text { .. }),
                    |cell| cell.flex_1().min_w_0(),
                )
                .when(matches!(child, Widget::Button { .. }), |cell| {
                    cell.flex_none()
                })
                .child(cell)
                .into_any_element(),
            _ => cell,
        };
        cells.push(cell);
    }
    if tools {
        return div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_0p5()
                    .p_0p5()
                    .rounded_lg()
                    .border_1()
                    .border_color(border)
                    .bg(bar)
                    .children(cells),
            )
            .into_any_element();
    }
    div()
        .w_full()
        .flex()
        .flex_row()
        .when(layout != RowLayout::Toolbar, |row| row.flex_wrap())
        .when(layout == RowLayout::Center, |row| row.justify_center())
        .items_center()
        .gap_1()
        .children(cells)
        .into_any_element()
}

/// The app's picture for an icon a plugin names; none for one this app does not know.
fn glyph(icon: ButtonIcon) -> Option<&'static str> {
    match icon {
        ButtonIcon::Home => Some(icons::HOME),
        ButtonIcon::Lock => Some(icons::LOCK),
        ButtonIcon::Power => Some(icons::POWER),
        ButtonIcon::Refresh => Some(icons::REFRESH),
        ButtonIcon::Unknown => None,
    }
}

/// A button drawn as an icon alone.
fn is_tool(widget: &Widget) -> bool {
    matches!(widget, Widget::Button { icon: Some(icon), .. } if glyph(*icon).is_some())
}

/// A quiet icon button whose ground turns to the danger color under the pointer.
fn warning_tool(cx: &App) -> ButtonCustomVariant {
    let danger = cx.theme().danger;
    ButtonCustomVariant::new(cx)
        .hover(danger.opacity(0.18))
        .active(danger.opacity(0.28))
}

/// A short upright line between buttons.
fn rule(color: Hsla) -> AnyElement {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(16.))
        .mx_1()
        .bg(color)
        .into_any_element()
}

fn dependency_action(
    ws: &Workspace,
    key: &Key,
    dependency: PhoneDependency,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    if ws.plugins.dependency_installs.contains(key) {
        return crate::ui::loading(dependency.progress(), cx).into_any_element();
    }
    let key = key.clone();
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap_2()
        .child(
            Button::new(eid(&key, "dependency", "install"))
                .primary()
                .label(dependency.label())
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.install_phone_dependency(key.clone(), cx);
                })),
        )
        .into_any_element()
}

fn text(text: &str, tone: Tone, size: Size, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let color = match tone {
        Tone::Normal => theme.foreground,
        Tone::Muted => theme.muted_foreground,
        Tone::Good => theme.success,
        Tone::Danger => theme.danger,
    };
    let line = div().text_color(color).child(text.to_owned());
    match size {
        Size::Title => line.title(),
        Size::Body => line.body(),
        Size::Small => line.small(),
    }
    .into_any_element()
}

/// A live picture in the widget's own units, in a phone when it asks; a click is a tap
/// and a drag a swipe.
fn screen(
    ws: &Workspace,
    key: &Key,
    id: &str,
    units: (f32, f32),
    in_phone: bool,
    room: Room,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let slot: Slot = (key.clone(), id.to_owned());
    let Some(drawn) = ws.plugins.drawn.get(&slot) else {
        // Its place is measured from the next draw.
        return div().into_any_element();
    };
    let body = phone::fit(room.width, room.height, units, PHONE);
    // A picture is not clipped by its frame's corners: it is rounded as the frame is.
    let corner = if in_phone {
        px(body * phone::CORNER)
    } else {
        px(8.)
    };
    let content = match ws.plugins.frames.get(&slot) {
        Some(picture) => img(ImageSource::Render(picture.clone()))
            .size_full()
            .rounded(corner)
            .object_fit(ObjectFit::Fill)
            .into_any_element(),
        None => phone::centered(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child("Waiting for the screen"),
        ),
    };
    let focus = ws.plugins.screen_focus.get(&slot).cloned();
    if in_phone {
        let screen = phone::screen(eid(&slot.0, "screen", id), body, units, content, drawn);
        let screen = touchable(screen, &slot, units, focus, cx);
        return div()
            .w_full()
            .flex()
            .justify_center()
            .child(phone::body(body, units, screen))
            .into_any_element();
    }
    let width = room
        .width
        .min(units.0)
        .min(room.height * units.0 / units.1)
        .max(1.);
    let screen = div()
        .id(eid(&slot.0, "screen", id))
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(width * units.1 / units.0))
        .rounded_lg()
        .overflow_hidden()
        .bg(rgb(0x000000))
        .child(content)
        .child(phone::measure(drawn));
    touchable(screen, &slot, units, focus, cx).into_any_element()
}

/// A screen the pointer and keys drive: a press, its moves and its lift go to the
/// plugin as they happen, so a held press is a long press and a drag follows the
/// pointer; the wheel scrolls, and keys typed while it has focus go to the device.
fn touchable(
    screen: Stateful<Div>,
    slot: &Slot,
    units: (f32, f32),
    focus: Option<FocusHandle>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let touch = |phase: TouchPhase| {
        let slot = slot.clone();
        move |ws: &mut Workspace, position: Point<Pixels>, cx: &mut Context<Workspace>| {
            let Some(drawn) = ws.plugins.drawn.get(&slot) else {
                return;
            };
            let at = match phase {
                TouchPhase::Down => phone::units_at(drawn, position, units),
                _ => phone::units_held(drawn, position, units),
            };
            let Some((x, y)) = at else { return };
            match phase {
                TouchPhase::Down => ws.plugins.pressed = Some((slot.clone(), (x, y))),
                TouchPhase::Move => match &mut ws.plugins.pressed {
                    Some((pressed, last)) if *pressed == slot && *last != (x, y) => *last = (x, y),
                    _ => return,
                },
                TouchPhase::Up => match ws.plugins.pressed.take() {
                    Some((pressed, _)) if pressed == slot => {}
                    other => {
                        ws.plugins.pressed = other;
                        return;
                    }
                },
            }
            ws.plugin_event(&slot.0, &slot.1, UiEvent::Touch { phase, x, y }, cx);
        }
    };
    let (down, moved, up, up_out) = (
        touch(TouchPhase::Down),
        touch(TouchPhase::Move),
        touch(TouchPhase::Up),
        touch(TouchPhase::Up),
    );
    let focused = focus.clone();
    let scrolled = slot.clone();
    let typed = slot.clone();
    let mut screen = screen
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &MouseDownEvent, window, cx| {
                if let Some(focus) = &focused {
                    window.focus(focus, cx);
                }
                down(ws, event.position, cx);
            }),
        )
        .on_mouse_move(cx.listener(move |ws, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                moved(ws, event.position, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |ws, event: &MouseUpEvent, _, cx| up(ws, event.position, cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |ws, event: &MouseUpEvent, _, cx| up_out(ws, event.position, cx)),
        )
        .on_scroll(cx.listener(move |ws, event: &ScrollEvent, _, cx| {
            let Some(drawn) = ws.plugins.drawn.get(&scrolled) else {
                return;
            };
            let by = event.delta.pixel_delta(px(16.));
            let (Some((x, y)), Some((dx, dy))) = (
                phone::units_at(drawn, event.position, units),
                phone::units_by(drawn, by, units),
            ) else {
                return;
            };
            if (dx, dy) != (0, 0) {
                let event = UiEvent::Scroll { x, y, dx, dy };
                ws.plugin_event(&scrolled.0, &scrolled.1, event, cx);
            }
        }))
        .on_key_down(cx.listener(move |ws, event: &KeyDownEvent, _, cx| {
            if let Some(event) = key_event(&event.keystroke, cx) {
                ws.plugin_event(&typed.0, &typed.1, event, cx);
                cx.stop_propagation();
            }
        }));
    if let Some(focus) = &focus {
        screen = screen.track_focus(focus);
    }
    screen
}

/// What a key pressed on a screen sends: a named key, a shortcut the device knows, the
/// pasteboard's text for a paste, or the text the key types.
fn key_event(keystroke: &Keystroke, cx: &App) -> Option<UiEvent> {
    let modifiers = &keystroke.modifiers;
    let named = |key: &str| Some(UiEvent::Key { key: key.into() });
    if modifiers.platform {
        return match (keystroke.key.as_str(), modifiers.shift) {
            ("a", false) => named("select_all"),
            ("z", false) => named("undo"),
            ("z", true) => named("redo"),
            ("v", false) => cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .filter(|text| !text.is_empty())
                .map(|text| UiEvent::Text { text }),
            _ => None,
        };
    }
    if modifiers.control || modifiers.alt {
        return None;
    }
    let key = match keystroke.key.as_str() {
        "enter" => "return",
        "backspace" => "delete",
        "delete" => "forward_delete",
        "tab" => "tab",
        "escape" => "escape",
        "left" => "left",
        "right" => "right",
        "up" => "up",
        "down" => "down",
        "home" => "home",
        "end" => "end",
        "pageup" => "page_up",
        "pagedown" => "page_down",
        _ => {
            return keystroke
                .key_char
                .clone()
                .filter(|text| !text.is_empty())
                .map(|text| UiEvent::Text { text });
        }
    };
    named(key)
}
