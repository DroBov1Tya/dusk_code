//! A provider card's models, folded under one row: a switch each says whether More models
//! offers it. A long list gets its own search, and the buttons act on what it shows.

use dusk_protocol::model::{Model, ProviderStatus};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// Lists longer than this get a search box.
const SEARCH_FROM: usize = 12;

/// The fold of a provider's models, or nothing while it has none listed.
pub fn section(
    ws: &Workspace,
    provider: &ProviderStatus,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let models = provider
        .models
        .as_ref()
        .ok()
        .filter(|models| !models.is_empty())?;
    if !provider.enabled {
        return None;
    }
    let name = provider.name.clone();
    let open = ws.providers_page.models_open.as_deref() == Some(name.as_str());
    let shown = models
        .iter()
        .filter(|model| !provider.is_hidden(&model.id))
        .count();
    let hint = cx.theme().muted_foreground;
    let toggled = name.clone();
    let head = widgets::row(
        SharedString::from(format!("provider-models-{name}")),
        false,
        cx,
    )
    .mx_0()
    .px_1()
    .role(Role::Button)
    .aria_label(format!("Models of {name}"))
    .child(widgets::icon(
        if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        },
        hint,
    ))
    .child(div().flex_1().small().child("Models in More models"))
    .child(
        div()
            .small()
            .text_color(hint)
            .child(format!("{shown} of {}", models.len())),
    )
    .on_click(cx.listener(move |ws, _, window, cx| {
        let page = &mut ws.providers_page;
        page.models_open = match page.models_open.as_deref() == Some(toggled.as_str()) {
            true => None,
            false => Some(toggled.clone()),
        };
        page.models_filter
            .update(cx, |filter, cx| filter.set_value("", window, cx));
        cx.notify();
    }));
    let mut fold = div().flex().flex_col().gap_1().child(head);
    if !open {
        return Some(fold);
    }
    let query = match models.len() > SEARCH_FROM {
        true => ws
            .providers_page
            .models_filter
            .read(cx)
            .value()
            .trim()
            .to_lowercase(),
        false => String::new(),
    };
    let listed: Vec<&Model> = models
        .iter()
        .filter(|model| model.id.to_lowercase().contains(&query))
        .collect();
    let ids: Vec<String> = listed.iter().map(|model| model.id.clone()).collect();
    let (show, hide) = ((name.clone(), ids.clone()), (name.clone(), ids));
    let what = if query.is_empty() { "all" } else { "these" };
    fold = fold.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .when(models.len() > SEARCH_FROM, |row| {
                row.child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.providers_page.models_filter)
                            .small()
                            .aria_label("Find a model"),
                    ),
                )
            })
            .when(models.len() <= SEARCH_FROM, |row| row.child(div().flex_1()))
            .child(
                Button::new(SharedString::from(format!("provider-models-show-{name}")))
                    .ghost()
                    .small()
                    .label(format!("Show {what}"))
                    .disabled(listed.is_empty())
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.set_models_hidden(&show.0, show.1.clone(), false, cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("provider-models-hide-{name}")))
                    .ghost()
                    .small()
                    .label(format!("Hide {what}"))
                    .disabled(listed.is_empty())
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.set_models_hidden(&hide.0, hide.1.clone(), true, cx)
                    })),
            ),
    );
    let mut list = div()
        .id(SharedString::from(format!("provider-models-list-{name}")))
        .flex()
        .flex_col()
        .max_h(px(320.))
        .overflow_y_scroll();
    if listed.is_empty() {
        list = list.child(widgets::muted("No models match", cx));
    }
    for model in listed {
        let on = !provider.is_hidden(&model.id);
        let (provider_name, id) = (name.clone(), model.id.clone());
        list = list.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_1()
                .h(px(32.))
                .flex_none()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .small()
                        .when(!on, |label| label.text_color(hint))
                        .child(model.id.clone()),
                )
                .child(
                    Switch::new(SharedString::from(format!(
                        "model-shown-{name}/{}",
                        model.id
                    )))
                    .checked(on)
                    .small()
                    .tooltip(if on {
                        "Offered in More models"
                    } else {
                        "Hidden from More models"
                    })
                    .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                        ws.set_models_hidden(&provider_name, vec![id.clone()], !*on, cx)
                    })),
                ),
        );
    }
    Some(fold.child(list))
}

impl Workspace {
    /// Hides models of a provider from More models or shows them again, and keeps the list
    /// in step once the core has saved it.
    fn set_models_hidden(
        &mut self,
        provider: &str,
        models: Vec<String>,
        hidden: bool,
        cx: &mut Context<Self>,
    ) {
        let (asked, changed, saved) = (provider.to_owned(), provider.to_owned(), models.clone());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .settings()
                    .models_hidden(&asked, models, hidden)
                    .await
            },
            move |ws, result, cx| match result {
                Ok(()) => {
                    if let Some(status) = ws.providers.iter_mut().find(|p| p.name == changed) {
                        status.hidden.retain(|id| !saved.contains(id));
                        if hidden {
                            status.hidden.extend(saved);
                        }
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
