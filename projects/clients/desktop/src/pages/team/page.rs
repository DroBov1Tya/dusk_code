//! The Team page: Back, how the team stands and why, its budget and the buttons that
//! steer it, the switch of layers, and the layers themselves.

use dusk_protocol::thread::{
    BudgetPart, Team, TeamEnd,
    team_layout::{self, Related},
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Layer, people, plan};
use crate::{ui, ui::Type, workspace::Workspace};

pub fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let body = match ws.shown_chat() {
        None => ui::empty(
            IconName::Bot,
            "No chat is open. The team of its plan shows here.",
            None,
            cx,
        ),
        Some(chat) if chat.team.is_none() && chat.tasks.is_empty() => empty(cx),
        Some(_) => team(ws, cx),
    };
    let title = ws
        .shown_chat()
        .and_then(|_| ws.threads.iter().find(|t| Some(&t.id) == ws.shown.as_ref()))
        .and_then(|thread| thread.title.clone())
        .unwrap_or_default();
    div().id("team-page").size_full().overflow_y_scroll().child(
        div()
            .max_w(px(1040.))
            .mx_auto()
            .px_8()
            .py_6()
            .flex()
            .flex_col()
            .child(
                div().pb_2().flex().flex_row().child(
                    Button::new("team-back")
                        .ghost()
                        .small()
                        .icon(IconName::ArrowLeft)
                        .label("Back")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_team(cx))),
                ),
            )
            .child(header(ws, title, cx))
            .child(body),
    )
}

/// A TEAM chat with no plan yet.
fn empty(cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .pt_6()
        .small()
        .text_color(muted)
        .child("No team yet. In TEAM mode the agent writes a plan; `run` starts a team here.")
}

/// "Team", the chat's title, how the team stands, and what steers it.
fn header(ws: &Workspace, title: String, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let team = ws.shown_chat().and_then(|chat| chat.team.clone());
    let status = team.as_ref().map(|team| {
        let (text, color) = match team.ended() {
            None => (team.progress_note(), theme.foreground),
            Some(TeamEnd::Done) => (team.progress_note(), theme.success),
            Some(_) => (team.progress_note(), theme.warning),
        };
        div().flex_none().small().text_color(color).child(text)
    });
    let actions = team.map(|team| steer(team, cx));
    div()
        .pb_2()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(div().flex_none().title().child("Team"))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .small()
                .text_color(muted)
                .truncate()
                .child(title),
        )
        .children(status)
        .children(actions)
}

/// Edit budget and Stop team while it runs. Once it ended short of done, Edit budget
/// alone: the team runs again when the lead runs the plan again.
fn steer(team: Team, cx: &mut Context<Workspace>) -> Div {
    let row = div().flex_none().flex().flex_row().gap_2();
    if team.ended() == Some(TeamEnd::Done) {
        return row;
    }
    let running = team.ended().is_none();
    row.child(
        Button::new("team-page-budget")
            .outline()
            .small()
            .label("Edit budget")
            .on_click(cx.listener(move |ws, _, window, cx| ws.ask_team_budget(&team, window, cx))),
    )
    .when(running, |row| {
        row.child(
            Button::new("team-page-stop")
                .danger()
                .small()
                .label("Stop team")
                .on_click(cx.listener(|ws, _, _, cx| ws.ask_stop_team(cx))),
        )
    })
}

/// One part of the budget: its name, a thin bar, and "used of all".
fn meter(part: BudgetPart, cx: &App) -> Div {
    let theme = cx.theme();
    let fill = match part.share() {
        share if share >= 1. => theme.danger,
        _ if part.near() => theme.warning,
        _ => theme.foreground.opacity(0.7),
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .meta()
        .text_color(theme.muted_foreground)
        .child(part.name)
        .child(
            div()
                .w(px(96.))
                .h(px(4.))
                .rounded_full()
                .bg(theme.border)
                .overflow_hidden()
                .child(div().h_full().w(px(96. * part.share())).bg(fill)),
        )
        .child(
            div()
                .text_color(theme.foreground.opacity(0.8))
                .child(part.text()),
        )
}

fn team(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws.shown_chat() else {
        return div();
    };
    let theme = cx.theme();
    let (muted, foreground, accent, border) = (
        theme.muted_foreground,
        theme.foreground,
        theme.primary,
        theme.border,
    );
    let page = &ws.team_page;
    let related = Related::of(page.pick(), chat.team.as_ref(), &chat.tasks);
    let mut body = div().flex().flex_col();
    if let Some(team) = &chat.team {
        let at_once = format!("{} of {} at once", team.used.at_once, team.budget.at_once());
        body = body.child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_x_6()
                .gap_y_1()
                .pb_2()
                .children(team.budget_parts().map(|part| meter(part, cx)))
                .child(div().meta().text_color(muted).child(at_once)),
        );
    }
    let tab = |id: &'static str, label: &'static str, layer: Layer, cx: &mut Context<Workspace>| {
        let on = page.layer == layer;
        div()
            .id(id)
            .h(px(28.))
            .px_0p5()
            .flex()
            .items_center()
            .small()
            .cursor_pointer()
            .border_b_2()
            .border_color(if on { accent } else { transparent_black() })
            .text_color(if on { foreground } else { muted })
            .child(label)
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.team_page.layer = layer;
                cx.notify();
            }))
    };
    body = body.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(border)
            .child(tab("team-layer-both", "Both", Layer::Both, cx))
            .child(tab("team-layer-people", "People", Layer::People, cx))
            .child(tab("team-layer-plan", "Plan", Layer::Plan, cx))
            .child(
                div()
                    .ml_auto()
                    .meta()
                    .text_color(muted)
                    .child("point at a node: both layers answer \u{00B7} click: a member's chat, a task's card"),
            ),
    );
    let show_people = page.layer != Layer::Plan;
    let show_plan = page.layer != Layer::People;
    if show_people {
        let layer = match &chat.team {
            Some(team) => {
                let layout = team_layout::people(team, &chat.tasks, &chat.agents);
                let caption = "the lead, its members, and who waits on whom \u{00B7} a node shrinks to one line when its task is done";
                section(
                    "team-people",
                    "People",
                    caption,
                    None,
                    people::render(layout, team, related.as_ref(), cx),
                    cx,
                )
            }
            None => section(
                "team-people",
                "People",
                "",
                None,
                div()
                    .small()
                    .text_color(muted)
                    .child("The plan is written; its members appear here when it runs."),
                cx,
            ),
        };
        body = body.child(layer);
    }
    if show_plan {
        let layout = team_layout::plan(&chat.tasks, page.unfold);
        let fold = layout.foldable.then(|| {
            let unfold = page.unfold;
            div()
                .id("team-plan-unfold")
                .ml_auto()
                .meta()
                .text_color(muted)
                .cursor_pointer()
                .hover(|link| link.text_color(cx.theme().foreground))
                .child(if unfold {
                    "fold done waves"
                } else {
                    "show done waves"
                })
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.team_page.unfold = !ws.team_page.unfold;
                    cx.notify();
                }))
        });
        let caption = "by dependency depth: a task stands right of everything it needs";
        body = body.child(section(
            "team-plan",
            "Plan",
            caption,
            fold,
            plan::render(layout, related.as_ref(), cx),
            cx,
        ));
    }
    body
}

/// A layer: its name and caption over it, and the layer in a frame that scrolls across
/// when it is wider than the page.
fn section(
    id: &'static str,
    name: &'static str,
    caption: &'static str,
    aside: Option<Stateful<Div>>,
    layer: Div,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .pt_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(div().small().font_weight(FontWeight::MEDIUM).child(name))
                .child(div().meta().text_color(muted).truncate().child(caption))
                .children(aside),
        )
        .child(
            div()
                .id(id)
                .w_full()
                .overflow_x_scroll()
                .pb_1()
                .child(layer),
        )
}
