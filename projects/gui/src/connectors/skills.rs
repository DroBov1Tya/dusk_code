//! Skills as cards, with their source and switch.

use dusk_protocol::{catalog::SkillKind, client::Switch as CoreSwitch, settings::Settings};
use gpui_kit::{component::ActiveTheme, *};

use super::{card, grid, matches, toggle};
use crate::{widgets, workspace::Workspace};

pub fn render(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let danger = cx.theme().danger;
    let mut cards = grid();
    let mut any = false;
    for skill in settings
        .skills
        .iter()
        .filter(|skill| matches(query, &[&skill.name, &skill.description]))
    {
        any = true;
        let kind = match skill.kind {
            SkillKind::Builtin => "Skill · built in".to_owned(),
            SkillKind::Own => "Skill · yours".to_owned(),
            SkillKind::Other => format!("Skill · {}", skill.source.as_deref().unwrap_or("other")),
        };
        cards = cards.child(card::render(
            card::Card {
                id: SharedString::from(format!("skill-card-{}", skill.name)),
                name: skill.name.clone(),
                kind,
                description: skill.problem.clone().unwrap_or_else(|| skill.description.clone()),
                state: skill.problem.as_ref().map(|_| ("Problem".to_owned(), danger)),
            },
            toggle(
                format!("skill-{}", skill.name),
                skill.enabled,
                CoreSwitch::Skill(skill.name.clone()),
                cx,
            ),
            cx,
        ));
    }
    if any { cards } else { div().child(widgets::muted("No skills match.", cx)) }
}
