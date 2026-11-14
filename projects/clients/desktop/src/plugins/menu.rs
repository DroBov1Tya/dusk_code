//! The menu of every part, behind the Plugins button in the header: a search on top,
//! then a fold per plugin with its parts, each with its icon, its title, where it opens
//! and a pin. Few plugins start unfolded, many folded, and a search unfolds what it finds.

use std::time::{Duration, Instant};

use dusk_protocol::plugin::{Part, Place};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable, input::Input, tooltip::Tooltip},
    *,
};

use super::{Key, place};
use crate::{
    ui::{Type, icons, size, widgets},
    workspace::Workspace,
};

const WIDTH: f32 = 360.;
const HEIGHT: f32 = 520.;
/// Up to this many plugins every fold starts open.
const OPEN_UP_TO: usize = 6;
/// A click on the button this soon after a click outside closed the menu is that click.
const REOPEN: Duration = Duration::from_millis(250);

impl Workspace {
    pub fn toggle_plugins_menu(&mut self, cx: &mut Context<Self>) {
        let plugins = &mut self.plugins;
        let just_closed = plugins
            .menu_closed
            .is_some_and(|closed| closed.elapsed() < REOPEN);
        plugins.menu = !plugins.menu && !just_closed;
        cx.notify();
    }

    fn close_plugins_menu(&mut self, cx: &mut Context<Self>) {
        self.plugins.menu = false;
        self.plugins.menu_closed = Some(Instant::now());
        cx.notify();
    }
}

/// The menu under the header's right end, while it is open.
pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Deferred> {
    if !ws.plugins.menu {
        return None;
    }
    let theme = cx.theme();
    let (border, surface) = (theme.border, theme.popover);
    let query = ws.plugins.search.read(cx).value().trim().to_lowercase();
    let groups = groups(ws, &query);
    let many = groups.len() > OPEN_UP_TO;
    let mut list = div()
        .id("plugins-menu-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .p_1()
        .flex()
        .flex_col();
    if groups.is_empty() {
        list = list.child(widgets::muted("No plugin part matches.", cx));
    }
    for (plugin, parts) in groups {
        let open = !query.is_empty() || (many == ws.plugins.turned.contains(plugin));
        list = list.child(fold(plugin, parts.len(), open, cx));
        if open {
            for part in parts {
                list = list.child(row(ws, part, cx));
            }
        }
    }
    let panel = div()
        .id("plugins-menu")
        .absolute()
        .top(px(crate::columns::topbar::HEIGHT - 6.))
        .right(px(12.))
        .w(px(WIDTH))
        .max_h(px(HEIGHT))
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .shadow_lg()
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| ws.close_plugins_menu(cx)))
        .child(
            div()
                .p_2()
                .border_b_1()
                .border_color(border)
                .child(Input::new(&ws.plugins.search).small()),
        )
        .child(list);
    Some(widgets::on_top(widgets::appear("plugins-menu-open", panel)))
}

/// The offered parts that match the search, by plugin in the status's order.
fn groups<'a>(ws: &'a Workspace, query: &str) -> Vec<(&'a str, Vec<&'a Part>)> {
    let mut groups: Vec<(&str, Vec<&Part>)> = Vec::new();
    let matches = |part: &Part| {
        query.is_empty()
            || part.title.to_lowercase().contains(query)
            || part.plugin.to_lowercase().contains(query)
    };
    for part in place::offered(ws).filter(|part| matches(part)) {
        match groups.iter_mut().find(|(plugin, _)| *plugin == part.plugin) {
            Some((_, parts)) => parts.push(part),
            None => groups.push((part.plugin.as_str(), vec![part])),
        }
    }
    groups
}

/// A plugin's name over its parts; a click folds or unfolds them.
fn fold(plugin: &str, count: usize, open: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let name = plugin.to_owned();
    widgets::menu_item(ElementId::Name(format!("plugins-fold:{plugin}").into()), cx)
        .child(widgets::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            muted,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .group_name()
                .child(plugin.to_owned()),
        )
        .child(div().meta().text_color(muted).child(count.to_string()))
        .on_click(cx.listener(move |ws, _, _, cx| {
            let turned = &mut ws.plugins.turned;
            if !turned.remove(&name) {
                turned.insert(name.clone());
            }
            cx.notify();
        }))
}

/// One part: a click opens it; the pin keeps its icon in its place.
fn row(ws: &Workspace, part: &Part, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    let key = Key::of(part);
    let pinned = place::is_pinned(ws, part, cx);
    let (accent, hover) = (theme.primary, theme.list_hover);
    let pin = (part.place != Place::Settings).then(|| {
        let part = part.clone();
        let tip = if pinned {
            "Unpin: reach it from this menu only"
        } else {
            "Pin its icon to its place"
        };
        div()
            .id(ElementId::Name(format!("plugins-pin:{}", key.pin()).into()))
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .hover(move |pin| pin.bg(hover))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .child(
                Icon::default()
                    .path(icons::PIN)
                    .size(px(14.))
                    .text_color(if pinned { accent } else { muted.opacity(0.6) }),
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                cx.stop_propagation();
                place::toggle_pin(ws, &part, cx);
                cx.notify();
            }))
    });
    let open = key.clone();
    widgets::menu_item(
        ElementId::Name(format!("plugins-part:{}", key.pin()).into()),
        cx,
    )
    .pl(px(size::ICON + 12.))
    .child(place::icon(part, size::ICON, foreground))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .body()
            .child(part.title.clone()),
    )
    .child(
        div()
            .flex_none()
            .meta()
            .text_color(muted)
            .child(place::opens_label(part)),
    )
    .child(div().flex_none().w(px(24.)).children(pin))
    .on_click(cx.listener(move |ws, _, _, cx| ws.open_plugin_part(open.clone(), cx)))
}
