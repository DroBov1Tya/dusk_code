//! A view's `List`: rows to choose from, or a menu. A menu is a full-width box in a column
//! and a compact title with its detail under it in a toolbar, as a device picker is.

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::plugin::{ListItem, UiEvent};
use gpui_kit::{
    component::{ActiveTheme, Icon},
    prelude::FluentBuilder,
    *,
};

use super::{Key, Slot, view::eid};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

const CHEVRON: &str = "icons/chevron-down.svg";
/// The narrowest a compact menu's items open, so short titles keep room for details.
const MENU_MIN: f32 = 220.;

/// Rows to choose from, the chosen one marked.
pub fn list(
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

/// The chosen item on a button; a click opens the others under it. `compact` draws the
/// button as a borderless title over its detail, sized to its text.
pub fn dropdown(
    ws: &Workspace,
    key: &Key,
    id: &str,
    items: &[ListItem],
    selected: Option<&str>,
    compact: bool,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let slot: Slot = (key.clone(), id.to_owned());
    let open = ws.plugins.open_list.as_ref() == Some(&slot);
    let chosen = selected.and_then(|selected| items.iter().find(|item| item.id == selected));
    let toggled = slot.clone();
    let face = match compact {
        true => compact_face(key, id, chosen, cx),
        false => boxed_face(key, id, chosen, cx),
    }
    .on_click(cx.listener(move |ws, _, _, cx| {
        let open = ws.plugins.open_list.as_ref() == Some(&toggled);
        ws.plugins.open_list = (!open).then(|| toggled.clone());
        cx.notify();
    }));
    let menu = open.then(|| menu(key, id, items, selected, compact, cx));
    div()
        .relative()
        .when(compact, |picker| picker.min_w_0().max_w_full())
        .when(!compact, |picker| picker.w_full())
        .child(face)
        .children(menu)
        .into_any_element()
}

/// A field-like box across the column with the chosen title.
fn boxed_face(key: &Key, id: &str, chosen: Option<&ListItem>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (border, muted, hover) = (theme.border, theme.muted_foreground, theme.accent);
    div()
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
            div().flex_1().min_w_0().truncate().body().child(
                chosen
                    .map_or("Choose", |item| item.title.as_str())
                    .to_owned(),
            ),
        )
        .child(Icon::default().path(CHEVRON).size_4().text_color(muted))
}

/// The chosen title, its detail small under it and a small chevron, with no border.
fn compact_face(key: &Key, id: &str, chosen: Option<&ListItem>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.accent);
    let title = chosen
        .map_or("Choose", |item| item.title.as_str())
        .to_owned();
    let detail = chosen
        .map(|item| item.detail.clone())
        .filter(|detail| !detail.is_empty());
    div()
        .id(eid(key, "menu", id))
        .max_w_full()
        .min_h(px(size::CONTROL))
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_0p5()
        // The title lines up with the text under it; the hover ground reaches past it.
        .ml(px(-8.))
        .rounded_lg()
        .cursor_pointer()
        .hover(move |button| button.bg(hover))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .children(
                    detail.map(|detail| div().truncate().meta().text_color(muted).child(detail)),
                ),
        )
        .child(
            Icon::default()
                .path(CHEVRON)
                .size_3p5()
                .flex_none()
                .text_color(muted),
        )
}

/// The items under the button, on top of the view; a click outside closes them.
fn menu(
    key: &Key,
    id: &str,
    items: &[ListItem],
    selected: Option<&str>,
    compact: bool,
    cx: &mut Context<Workspace>,
) -> Deferred {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mut menu = div()
        .id(eid(key, "menu-items", id))
        .absolute()
        .top_full()
        .mt_1()
        .left_0()
        .when(compact, |menu| menu.min_w(px(MENU_MIN)))
        .when(!compact, |menu| menu.right_0())
        .max_h(px(280.))
        .overflow_y_scroll()
        .p_1()
        .rounded_lg()
        .elevation(Level::Floating, cx)
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
                    row.child(
                        div()
                            .flex_none()
                            .small()
                            .text_color(muted)
                            .child(item.detail.clone()),
                    )
                })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.plugins.open_list = None;
                    let item = picked.clone();
                    ws.plugin_event(&key, &id, UiEvent::Select { item }, cx);
                })),
        );
    }
    widgets::on_top(menu)
}
