//! The More models submenu: every enabled provider, folded, with the models the user did not
//! hide, under a search that unfolds whatever matches.

use dusk_protocol::model::{Model, ProviderStatus};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
        input::Input,
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use super::{FIRST, ROW, back_row, catalog_status, model_row};
use crate::pages::chat::model_bar::{picker_hint, picker_panel};
use crate::ui::looping::Loop;
use crate::{
    ui::{Type, icons::REFRESH, widgets},
    workspace::Workspace,
};

/// The models of a provider More models offers: all but the hidden ones, and the one in use
/// even when hidden, so the menu never loses the current model.
pub fn shown_models<'a>(provider: &'a ProviderStatus, current: &str) -> Vec<&'a Model> {
    let Ok(models) = &provider.models else {
        return Vec::new();
    };
    models
        .iter()
        .filter(|model| {
            !provider.is_hidden(&model.id)
                || current.split_once('/') == Some((provider.name.as_str(), model.id.as_str()))
        })
        .collect()
}

/// The search typed at the top of More models, lowercased; empty when there is none.
pub fn models_query(ws: &Workspace, cx: &App) -> String {
    ws.models_search.read(cx).value().trim().to_lowercase()
}

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
        .max_h(px(max_height - 54. - ROW))
        .flex()
        .flex_col()
        .overflow_y_scroll();
    if ws.providers.is_empty() {
        scroll = scroll.child(widgets::muted(catalog_status(ws), cx));
    }
    let (foreground, hint) = (cx.theme().foreground, picker_hint(cx));
    let query = models_query(ws, cx);
    let searching = !query.is_empty();
    let mut matched = false;
    // A provider switched off offers nothing, so it is not listed.
    for provider in ws.providers.iter().filter(|provider| provider.enabled) {
        let name = provider.name.clone();
        let pending = ws.models_pending.contains(&provider.name);
        // A provider's name matching keeps all its models; otherwise only the matching ones.
        let name_matches = provider.name.to_lowercase().contains(&query);
        let models: Vec<&Model> = shown_models(provider, current)
            .into_iter()
            .filter(|model| name_matches || model.id.to_lowercase().contains(&query))
            .collect();
        if searching && !name_matches && models.is_empty() {
            continue;
        }
        matched = true;
        let open = searching || ws.models_open.contains(&provider.name);
        let toggled = provider.name.clone();
        let count = match (&provider.models, pending) {
            (_, true) => String::from("..."),
            (Ok(_), _) => models.len().to_string(),
            (Err(_), _) => String::from("!"),
        };
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
            .child(div().small().text_color(hint).child(count))
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
        // Its own section says it is still being asked; the others are already usable.
        if ws.models_pending.contains(&provider.name) {
            scroll = scroll.child(widgets::muted("Loading the models...", cx));
            continue;
        }
        match &provider.models {
            Ok(listed) if listed.is_empty() => {
                scroll = scroll.child(widgets::muted("No listed models", cx));
            }
            Ok(_) if models.is_empty() => {
                scroll = scroll.child(widgets::muted("Every model is hidden in Settings", cx));
            }
            Ok(_) => {
                // The digits pick from the list as it stands unfiltered, as `first_models` does.
                let keyed = !searching && current.split('/').next() == Some(provider.name.as_str());
                for (index, model) in models.iter().enumerate() {
                    let name = format!("{}/{}", provider.name, model.id);
                    let key = (keyed && index < FIRST).then_some(index + 1);
                    scroll = scroll.child(model_row(&name, &model.id, current, key, cx).pl_6());
                }
            }
            Err(error) => scroll = scroll.child(widgets::muted(error.clone(), cx)),
        }
    }
    if searching && !matched {
        scroll = scroll.child(widgets::muted("No models match", cx));
    }
    // Beside the menu the row it opened from already names it; alone, it needs the way back.
    picker_panel(cx)
        .w(px(width))
        .when(!side_by_side, |list| {
            list.child(back_row("More models", side_by_side, cx))
        })
        .child(refresh_row(ws, cx))
        .child(
            div().px_1().pb_1().child(
                Styled::h(Input::new(&ws.models_search), px(ROW)).aria_label("Search models"),
            ),
        )
        .child(scroll)
}

/// Asks every provider for its models again, for a model added since the list came.
fn refresh_row(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let hint = picker_hint(cx);
    let control = match ws.models_loading {
        // The same arrows turn while the providers answer.
        true => div()
            .size(px(crate::ui::size::CONTROL))
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::default()
                    .path(REFRESH)
                    .text_color(hint)
                    .looping(std::time::Duration::from_millis(900), |icon, delta| {
                        icon.rotate(percentage(delta))
                    }),
            )
            .into_any_element(),
        false => Button::new("models-refresh")
            .ghost()
            .icon(Icon::default().path(REFRESH))
            .tooltip("Ask the providers for their models again")
            .on_click(cx.listener(|ws, _, _, cx| ws.load_models(cx)))
            .into_any_element(),
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .pl_3()
        .pr_1()
        .h(px(ROW))
        .child(div().small().text_color(hint).child("Providers"))
        .child(control)
}
