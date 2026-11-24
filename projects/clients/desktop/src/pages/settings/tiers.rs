//! The subagent models on the LLM page: the strong, default and weak models an agent may
//! give the subagents it asks, each one of the connected providers' models or the chat's
//! own. The core keeps them and says in the status which one lost its provider.

use dusk_protocol::config::{ModelTier, TierState};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::Button,
        menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
    *,
};

use super::{line, list};
use crate::{ui::Type, workspace::Workspace};

/// What an unset tier shows: the subagent answers on the model of the chat that asks.
const SAME: &str = "Same as the chat";

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(super::ai::section(
            "Subagent models",
            "The levels an agent can give the subagents it asks. A level left unset uses the chat's model.",
            cx,
        ))
        .child(list(
            ModelTier::ALL.map(|tier| row(ws, tier, ws.status.model_tiers.get(tier), cx)),
            cx,
        ))
}

fn row(ws: &Workspace, tier: ModelTier, state: &TierState, cx: &mut Context<Workspace>) -> Div {
    let (label, hint) = match tier {
        ModelTier::Strong => ("Strong", "Design, review and hard debugging"),
        ModelTier::Default => ("Default", "Most of the work an agent hands off"),
        ModelTier::Weak => ("Weak", "Mechanical work and read-only scouting"),
    };
    let missing = state.chosen.as_deref().filter(|_| state.missing);
    let warning = cx.theme().warning;
    let note = missing.map(|model| {
        let provider = model
            .split_once('/')
            .map_or(model, |(provider, _)| provider);
        div().small().text_color(warning).child(format!(
            "{provider} is not connected, so the chat's model answers"
        ))
    });
    line(label, hint, cx).child(
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap_1()
            .flex_shrink_0()
            .max_w(px(300.))
            .child(picker(ws, tier, state, cx))
            .children(note),
    )
}

/// The tier's model on a button; its menu offers the chat's model, then every enabled
/// provider's models in a submenu of its own.
fn picker(
    ws: &Workspace,
    tier: ModelTier,
    state: &TierState,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let chosen = state.chosen.clone();
    let label = chosen.clone().unwrap_or_else(|| SAME.to_owned());
    let providers: Vec<(String, Result<Vec<String>, String>)> = ws
        .providers
        .iter()
        .filter(|provider| provider.enabled)
        .map(|provider| {
            let models = provider
                .models
                .as_ref()
                .map(|models| models.iter().map(|model| model.id.clone()).collect())
                .map_err(Clone::clone);
            (provider.name.clone(), models)
        })
        .collect();
    let loading = ws.models_loading;
    let workspace = cx.weak_entity();
    let button = Button::new(SharedString::from(format!("tier-{}", tier.name())))
        .outline()
        .small()
        .label(label.clone())
        .dropdown_caret(true)
        .tooltip(format!("The {} subagent model", tier.name()))
        .when(state.missing, |button| button.icon(IconName::TriangleAlert))
        .dropdown_menu(move |menu, window, cx| {
            let handle = workspace.clone();
            let choose = move |model: Option<String>| {
                let workspace = handle.clone();
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                    let model = model.clone();
                    let _ = workspace.update(cx, |ws, cx| ws.set_model_tier(tier, model, cx));
                }
            };
            let mut menu = menu
                .min_w(px(220.))
                .max_h(px(360.))
                .scrollable(true)
                .item(
                    PopupMenuItem::new(SAME)
                        .checked(chosen.is_none())
                        .on_click(choose(None)),
                )
                .separator();
            if providers.is_empty() {
                menu = menu.label(if loading {
                    "Loading the models..."
                } else {
                    "Connect a provider to pick its models"
                });
            }
            for (provider, models) in &providers {
                let (provider, models) = (provider.clone(), models.clone());
                let chosen = chosen.clone();
                let choose = choose.clone();
                let holds = chosen
                    .as_deref()
                    .and_then(|chosen| chosen.split_once('/'))
                    .is_some_and(|(owner, _)| owner == provider);
                let title = if holds {
                    format!("{provider} \u{2713}")
                } else {
                    provider.clone()
                };
                menu = menu.submenu(title, window, cx, move |mut sub: PopupMenu, _, _| {
                    sub = sub.max_h(px(360.)).scrollable(true);
                    match &models {
                        Ok(models) if models.is_empty() => sub.label("No listed models"),
                        Ok(models) => {
                            for id in models {
                                let name = format!("{provider}/{id}");
                                let checked = chosen.as_deref() == Some(name.as_str());
                                sub = sub.item(
                                    PopupMenuItem::new(id.clone())
                                        .checked(checked)
                                        .on_click(choose(Some(name))),
                                );
                            }
                            sub
                        }
                        Err(error) => sub.label(error.clone()),
                    }
                });
            }
            menu
        });
    div().max_w_full().child(button)
}

impl Workspace {
    /// Saves a tier's model, `None` for the chat's; the status the core sends after
    /// shows it.
    pub fn set_model_tier(
        &mut self,
        tier: ModelTier,
        model: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.core.call(
            cx,
            move |client| async move { client.settings().model_tier(tier, model.as_deref()).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
