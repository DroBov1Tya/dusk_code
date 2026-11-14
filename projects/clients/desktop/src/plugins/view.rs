//! A part's view drawn with the app's own widgets and type scale. Text is text, never
//! markup; each widget that can be used sends its event back under its id.

use dusk_protocol::plugin::{ListItem, Size, Tone, UiEvent, Widget};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon,
        button::{Button, ButtonVariants},
        input::Input,
    },
    prelude::FluentBuilder,
    *,
};

use super::{Key, Slot};
use crate::{
    ui::{
        Type,
        phone::{self, Gesture},
        size, widgets,
    },
    workspace::Workspace,
};

/// The widest a phone is drawn, as the simulator draws its own.
const PHONE: f32 = 300.;
const CHEVRON: &str = "icons/chevron-down.svg";

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
        Some(root) => widget(ws, key, root, room, cx),
        None => crate::ui::loading("Opening", cx).into_any_element(),
    }
}

/// Every widget of a view, depth first.
pub fn each(widget: &Widget, visit: &mut impl FnMut(&Widget)) {
    visit(widget);
    if let Widget::Column { children } | Widget::Row { children } = widget {
        for child in children {
            each(child, visit);
        }
    }
}

/// An element id unique to the widget `id` of `key`.
fn eid(key: &Key, kind: &str, id: &str) -> ElementId {
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
        Widget::Row { children } => div()
            .w_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .children(
                children
                    .iter()
                    .map(|child| self::widget(ws, key, child, room, cx)),
            )
            .into_any_element(),
        Widget::Text { text, tone, size } => self::text(text, *tone, *size, cx),
        Widget::Button {
            id,
            label,
            primary,
            disabled,
        } => {
            let (key, id) = (key.clone(), id.clone());
            // A button keeps its own width; a column would stretch it across the panel.
            div()
                .flex()
                .child(
                    Button::new(eid(&key, "button", &id))
                        .label(label.clone())
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
        } => list(key, id, items, selected.as_deref(), cx),
        Widget::List {
            id,
            items,
            selected,
            menu: true,
        } => dropdown(ws, key, id, items, selected.as_deref(), cx),
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

/// Rows to choose from, the chosen one marked.
fn list(
    key: &Key,
    id: &str,
    items: &[ListItem],
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let mut rows = div()
        .id(eid(key, "list", id))
        .w_full()
        .max_h(px(320.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (index, item) in items.iter().enumerate() {
        let chosen = selected == Some(item.id.as_str());
        let (key, id, picked) = (key.clone(), id.to_owned(), item.id.clone());
        rows = rows.child(
            widgets::row(eid(&key, "row", &format!("{id}:{index}")), chosen, cx)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .body()
                        .child(item.title.clone()),
                )
                .when(!item.detail.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .max_w(px(160.))
                            .truncate()
                            .meta()
                            .text_color(muted)
                            .child(item.detail.clone()),
                    )
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    let item = picked.clone();
                    ws.plugin_event(&key, &id, UiEvent::Select { item }, cx);
                })),
        );
    }
    rows.into_any_element()
}

/// The chosen item on a button; a click opens the others under it.
fn dropdown(
    ws: &Workspace,
    key: &Key,
    id: &str,
    items: &[ListItem],
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (border, muted, hover, surface) = (
        theme.border,
        theme.muted_foreground,
        theme.accent,
        theme.popover,
    );
    let slot: Slot = (key.clone(), id.to_owned());
    let open = ws.plugins.open_list.as_ref() == Some(&slot);
    let chosen = selected
        .and_then(|selected| items.iter().find(|item| item.id == selected))
        .map_or("Choose", |item| item.title.as_str());
    let toggled = slot.clone();
    let button = div()
        .id(eid(key, "menu", id))
        .w_full()
        .h(px(size::CONTROL))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .cursor_pointer()
        .hover(move |button| button.bg(hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .body()
                .child(chosen.to_owned()),
        )
        .child(Icon::default().path(CHEVRON).size_4().text_color(muted))
        .on_click(cx.listener(move |ws, _, _, cx| {
            let open = ws.plugins.open_list.as_ref() == Some(&toggled);
            ws.plugins.open_list = (!open).then(|| toggled.clone());
            cx.notify();
        }));
    let menu = open.then(|| {
        let mut menu = div()
            .id(eid(key, "menu-items", id))
            .absolute()
            .top(px(size::CONTROL + 4.))
            .left_0()
            .right_0()
            .max_h(px(280.))
            .overflow_y_scroll()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(surface)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.plugins.open_list = None;
                cx.notify();
            }));
        for (index, item) in items.iter().enumerate() {
            let is_chosen = selected == Some(item.id.as_str());
            let (key, id, picked) = (key.clone(), id.to_owned(), item.id.clone());
            menu = menu.child(
                widgets::menu_item(eid(&key, "menu-item", &format!("{id}:{index}")), cx)
                    .when(is_chosen, |row| row.font_weight(FontWeight::SEMIBOLD))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .body()
                            .child(item.title.clone()),
                    )
                    .when(!item.detail.is_empty(), |row| {
                        row.child(div().small().text_color(muted).child(item.detail.clone()))
                    })
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.plugins.open_list = None;
                        let item = picked.clone();
                        ws.plugin_event(&key, &id, UiEvent::Select { item }, cx);
                    })),
            );
        }
        widgets::on_top(menu)
    });
    div()
        .relative()
        .w_full()
        .child(button)
        .children(menu)
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
    let corner = if in_phone { px(body * 0.13) } else { px(8.) };
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
    let (key, widget_id) = (key.clone(), id.to_owned());
    let on_click = cx.listener(move |ws, event: &ClickEvent, _, cx| {
        let slot = (key.clone(), widget_id.clone());
        let Some(drawn) = ws.plugins.drawn.get(&slot) else {
            return;
        };
        let event = match phone::gesture(event, drawn, units) {
            Some(Gesture::Tap(x, y)) => UiEvent::Tap { x, y },
            Some(Gesture::Swipe(from, to)) => UiEvent::Swipe { from, to },
            None => return,
        };
        ws.plugin_event(&key, &widget_id, event, cx);
    });
    if in_phone {
        let screen = phone::screen(eid(&slot.0, "screen", id), body, units, content, drawn)
            .cursor_pointer()
            .on_click(on_click);
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
    div()
        .id(eid(&slot.0, "screen", id))
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(width * units.1 / units.0))
        .rounded_lg()
        .overflow_hidden()
        .bg(rgb(0x000000))
        .cursor_pointer()
        .child(content)
        .child(phone::measure(drawn))
        .on_click(on_click)
        .into_any_element()
}
