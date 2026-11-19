//! Skills as rows of one card, with their source and switch, or the Workshop's single
//! skills to install. A pack is never offered here: it lives under Plugins, and its
//! skills show here once it is installed.

use dusk_protocol::{catalog::SkillKind, client::Switch as CoreSwitch, config::Settings};
use gpui_kit::{component::ActiveTheme, *};

use super::{
    card, grid, matches,
    plugins::{Scope, View, workshop},
    toggle,
};
use crate::{modal::detail::Detail, workspace::Workspace};

pub fn render(
    ws: &Workspace,
    settings: &Settings,
    query: &str,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let body = match ws.view(Scope::Skills) {
        View::Discover => workshop::render(ws, query, window, cx),
        View::Installed => installed(settings, query, cx),
    };
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(workshop::switch(ws, Scope::Skills, cx))
        .child(body)
}

fn installed(settings: &Settings, query: &str, cx: &mut Context<Workspace>) -> Div {
    let danger = cx.theme().danger;
    let mut cards = grid(cx);
    let mut any = false;
    for skill in settings
        .skills
        .iter()
        .filter(|skill| matches(query, &[&skill.name, &skill.description]))
    {
        any = true;
        let open = skill.name.clone();
        let kind = match skill.kind {
            SkillKind::Builtin => "Skill · built in".to_owned(),
            SkillKind::Own => "Skill · yours".to_owned(),
            SkillKind::Other => format!("Skill · {}", skill.source.as_deref().unwrap_or("other")),
        };
        cards = cards.child(
            card::render(
                card::Card {
                    id: SharedString::from(format!("skill-card-{}", skill.name)),
                    name: skill.name.clone(),
                    kind,
                    description: skill
                        .problem
                        .clone()
                        .unwrap_or_else(|| skill.description.clone()),
                    state: skill
                        .problem
                        .as_ref()
                        .map(|_| ("Problem".to_owned(), danger)),
                    icon: None,
                },
                toggle(
                    format!("skill-{}", skill.name),
                    skill.enabled,
                    CoreSwitch::Skill(skill.name.clone()),
                    cx,
                ),
                cx,
            )
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.open_detail(
                    Detail::Skill {
                        name: open.clone(),
                        file: None,
                    },
                    cx,
                );
            })),
        );
    }
    if any {
        cards
    } else {
        div().child(crate::ui::empty(
            gpui_kit::component::IconName::Search,
            "No skill matches that word.",
            Some(super::clear_search("skills-empty-clear", cx)),
            cx,
        ))
    }
}
