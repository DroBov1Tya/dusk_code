//! The More models submenu: every enabled provider, folded, with its models.

use gpui_kit::{
    component::{ActiveTheme, IconName, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use super::{ROW, back_row, catalog_status, model_row};
use crate::pages::chat::model_bar::{picker_hint, picker_panel};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn all_models(
    ws: &Workspace,
    current: &str,
    max_height: f32,
    width: f32,
    side_by_side: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut scroll = div()
        .id("all-models")
        .max_h(px(max_height - 54.))
        .flex()
        .flex_col()
        .overflow_y_scroll();
    if ws.providers.is_empty() {
        scroll = scroll.child(widgets::muted(catalog_status(ws), cx));
    }
    let (foreground, hint) = (cx.theme().foreground, picker_hint(cx));
    // A provider switched off offers nothing, so it is not listed.
    for provider in ws.providers.iter().filter(|provider| provider.enabled) {
        let name = provider.name.clone();
        let open = ws.models_open.contains(&provider.name);
        let toggled = provider.name.clone();
        let count = provider.models.as_ref().map_or(0, Vec::len);
        // The provider's name is the landmark of the list: larger and brighter than its
        // models, and it folds them, so two providers' models are never read as one.
        scroll = scroll.child(
            widgets::row(
                SharedString::from(format!("models-provider-{}", provider.name)),
                false,
                cx,
            )
            .role(Role::Button)
            .aria_label(provider.name.clone())
            .h(px(ROW + 4.))
            .child(widgets::icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                hint,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(foreground)
                    .child(provider.name.clone()),
            )
            .children(
                provider
                    .balance
                    .as_ref()
                    .and_then(|balance| balance.as_ref().ok())
                    .map(|balance| div().small().text_color(hint).child(balance.text())),
            )
            .child(div().small().text_color(hint).child(count.to_string()))
            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
            .on_click(cx.listener(move |ws, _, _, cx| {
                if !ws.models_open.remove(&toggled) {
                    ws.models_open.insert(toggled.clone());
                }
                cx.notify();
            })),
        );
        if !open {
            continue;
        }
        match &provider.models {
            Ok(models) if models.is_empty() => {
                scroll = scroll.child(widgets::muted("No listed models", cx));
            }
            Ok(models) => {
                for model in models {
                    let name = format!("{}/{}", provider.name, model.id);
                    scroll = scroll.child(model_row(&name, &model.id, current, None, cx).pl_6());
                }
            }
            Err(error) => scroll = scroll.child(widgets::muted(error.clone(), cx)),
        }
    }
    // Beside the menu the row it opened from already names it; alone, it needs the way back.
    picker_panel(cx)
        .w(px(width))
        .when(!side_by_side, |list| {
            list.child(back_row("More models", side_by_side, cx))
        })
        .child(scroll)
}
