//! The Settings page of the parts plugins place there: a row per part, and the chosen
//! one's view under them.

use dusk_protocol::plugin::Place;
use gpui_kit::{component::ActiveTheme, *};

use super::{Key, place, view};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The reading width of the settings page, less its padding.
const WIDTH: f32 = 816.;

/// Whether any plugin places a part under Settings, so the page is listed.
pub fn any(ws: &Workspace) -> bool {
    place::offered(ws).any(|part| part.place == Place::Settings)
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, foreground, muted, surface) = (
        theme.border,
        theme.foreground,
        theme.muted_foreground,
        theme.muted,
    );
    let parts: Vec<_> = place::offered(ws)
        .filter(|part| part.place == Place::Settings)
        .collect();
    if parts.is_empty() {
        return widgets::muted("No plugin adds settings.", cx);
    }
    let mut rows = div().flex().flex_col().gap_0p5();
    for part in parts {
        let key = Key::of(part);
        let chosen = ws.plugins.settings.as_ref() == Some(&key);
        rows = rows.child(
            widgets::row(
                ElementId::Name(format!("plugin-settings:{}", key.pin()).into()),
                chosen,
                cx,
            )
            .h(px(size::ROW))
            .child(place::icon(part, size::ICON, foreground))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .body()
                    .child(part.title.clone()),
            )
            .child(div().meta().text_color(muted).child(part.plugin.clone()))
            .on_click(cx.listener(move |ws, _, _, cx| ws.open_plugin_part(key.clone(), cx))),
        );
    }
    let shown = ws.plugins.settings.as_ref().map(|key| {
        let room = view::Room {
            width: WIDTH - 40.,
            height: f32::INFINITY,
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(surface)
            .child(
                div()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(super::title(ws, key)),
            )
            .child(view::render(ws, key, room, cx))
    });
    div().flex().flex_col().gap_4().child(rows).children(shown)
}
