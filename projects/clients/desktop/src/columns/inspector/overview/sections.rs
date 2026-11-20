//! The Overview's sections in their order. Each is absent when it has nothing to say;
//! the same section draws in the column and on the page, only its frame differs.

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::thread::{LogEntry, LogKind, Overview, PlanCounts, Team, ThreadTokens};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, button::Button},
    *,
};

use super::Shown;
use crate::{
    columns::inspector::context,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The column's width or the page's.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Narrow,
    Wide,
}

/// The log lines the column shows; the page shows all the Overview has.
const NARROW_LOG: usize = 8;
/// A budget part this used warns.
const NEAR: f32 = 0.8;

/// Every section the chat has something for, in order.
pub fn all(ws: &Workspace, form: Form, cx: &mut Context<Workspace>) -> Vec<Div> {
    let chat = ws.shown_chat();
    let shown = ws.overview.of(ws.shown.as_deref());
    let overview = shown.and_then(|shown| shown.overview.as_ref());
    [
        shown.and_then(|shown| progress(shown, form, cx)),
        shown
            .and_then(|shown| shown.team.as_ref())
            .and_then(|team| members(team, form, cx)),
        chat.and_then(|chat| chat.usage)
            .and_then(|usage| tokens(&usage, shown.and_then(|s| s.plan), form, cx)),
        ws.context.is_some().then(|| context_section(ws, form, cx)),
        overview.and_then(|overview| changes(overview, form, cx)),
        overview.and_then(|overview| activity(overview, form, cx)),
        shown.and_then(|shown| log(shown, form, cx)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A section's head, a name and a few words at the right, over its body.
fn frame(form: Form, title: &'static str, aside: Option<Div>, body: Div, cx: &App) -> Div {
    let theme = cx.theme();
    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(36.))
        .px_4()
        .child(
            div()
                .flex_1()
                .group_name()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .children(aside.map(|aside| aside.meta().text_color(theme.muted_foreground)));
    let section = div().flex().flex_col().child(head).child(body.px_4());
    match form {
        Form::Narrow => section.pb_3(),
        Form::Wide => section.pb_4().rounded_xl().elevation(Level::Raised, cx),
    }
}

/// A measured figure: the value, then what it is, small.
fn figure(value: String, label: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(rems(1.1))
                .font_weight(FontWeight::MEDIUM)
                .child(value),
        )
        .child(div().meta().text_color(theme.muted_foreground).child(label))
}

fn figures(items: Vec<Div>) -> Div {
    div().flex().flex_row().gap_6().children(items)
}

/// A 32 px row: a text that may be cut, then figures at the right.
fn row(text: Div, right: Vec<Div>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(32.))
        .child(text.flex_1().min_w_0().truncate())
        .children(right)
}

fn mono(text: String) -> Div {
    div().small().font_family("Menlo").child(text)
}

fn progress(shown: &Shown, form: Form, cx: &App) -> Option<Div> {
    let counts = shown.plan.filter(|counts| counts.total() > 0)?;
    let theme = cx.theme();
    let all = counts.total();
    let running = counts.at_work();
    let aside = shown
        .team
        .as_ref()
        .map(|team| div().child(team.status_line()));
    // A segment a status, as wide as its share, in the order done, at work, blocked, the
    // rest: the bar stays the same size whether the plan has 3 tasks or 300.
    let parts = [
        (counts.done, theme.success),
        (running + counts.review, theme.primary),
        (counts.blocked + counts.failed, theme.warning),
        (counts.open, theme.border),
    ];
    let segments = div()
        .flex()
        .flex_row()
        .w_full()
        .gap(px(2.))
        .my_3()
        .overflow_hidden()
        .children(
            parts
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, color)| {
                    div()
                        .w(relative(count as f32 / all as f32))
                        .min_w(px(3.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(color)
                }),
        );
    let mut body = div()
        .flex()
        .flex_col()
        .child(figures(vec![
            figure(format!("{} of {all}", counts.done), "tasks done", cx),
            figure(running.to_string(), "running", cx),
            figure(counts.blocked.to_string(), "blocked", cx),
        ]))
        .child(segments);
    if let Some(team) = &shown.team {
        let (budget, used) = (team.budget, team.used);
        body = body
            .child(budget_row(
                "Steps",
                used.steps.into(),
                budget.steps().into(),
                "",
                cx,
            ))
            .child(budget_row("Tokens", used.tokens, budget.tokens(), "", cx))
            .child(budget_row(
                "Time",
                used.minutes.into(),
                budget.minutes().into(),
                " min",
                cx,
            ));
    }
    Some(frame(form, "Progress", aside, body, cx))
}

/// One part of the team's budget: its name, a thin bar, and "used of all".
fn budget_row(name: &'static str, used: u64, all: u64, unit: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let share = (used as f32 / all.max(1) as f32).min(1.);
    let fill = if share >= NEAR {
        theme.warning
    } else {
        theme.muted_foreground
    };
    let amount = |value: u64| {
        if unit.is_empty() {
            widgets::short(value)
        } else {
            value.to_string()
        }
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(24.))
        .meta()
        .text_color(theme.muted_foreground)
        .child(div().w(px(64.)).child(name))
        .child(
            div()
                .flex_1()
                .h(px(4.))
                .rounded_full()
                .bg(theme.border)
                .child(div().h_full().w(relative(share)).rounded_full().bg(fill)),
        )
        .child(div().w(px(96.)).text_right().child(format!(
            "{} of {}{unit}",
            amount(used),
            amount(all)
        )))
}

fn members(team: &Team, form: Form, cx: &App) -> Option<Div> {
    if team.members.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let at_work = team.at_work();
    let mut body = div().flex().flex_col();
    for member in &team.members {
        let (word, color) = (
            member.status.word(),
            widgets::tone(member.status.tone(), cx),
        );
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .h(px(36.))
                .small()
                .child(widgets::icon(IconName::Bot, color))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(member.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .meta()
                        .text_color(theme.muted_foreground)
                        .child(member.role.clone().unwrap_or_else(|| "general".into())),
                )
                .child(div().meta().text_color(color).child(word)),
        );
    }
    let aside = div().child(format!("{at_work} of {} at work", team.members.len()));
    Some(frame(form, "Members", Some(aside), body, cx))
}

fn tokens(usage: &ThreadTokens, plan: Option<PlanCounts>, form: Form, cx: &App) -> Option<Div> {
    if usage.model_calls == 0 {
        return None;
    }
    let spent = usage.spent.prompt_tokens + usage.spent.completion_tokens;
    let cached = usage.spent.cached_prompt_tokens * 100 / usage.spent.prompt_tokens.max(1);
    let calls = widgets::count(usage.model_calls as usize, "call");
    let aside = match usage.cost() {
        Some(cost) => format!("{calls} \u{00B7} {cost}"),
        None => calls,
    };
    let mut items = vec![
        figure(widgets::short(spent), "spent", cx),
        figure(format!("{cached}%"), "from cache", cx),
    ];
    if let Some(done) = plan.map(|plan| plan.done).filter(|done| *done > 0) {
        items.push(figure(
            widgets::short(spent / u64::from(done)),
            "per task done",
            cx,
        ));
    }
    Some(frame(
        form,
        "Tokens",
        Some(div().child(aside)),
        figures(items),
        cx,
    ))
}

fn context_section(ws: &Workspace, form: Form, cx: &mut Context<Workspace>) -> Div {
    // The context draws its own rows; the frame gives it the head the others have.
    let body = context::render(ws, cx).px_0();
    frame(form, "Context", None, body, cx)
}

fn changes(overview: &Overview, form: Form, cx: &App) -> Option<Div> {
    let changes = &overview.changes;
    if changes.files == 0 {
        return None;
    }
    let theme = cx.theme();
    let (ok, bad, muted) = (theme.success, theme.danger, theme.muted_foreground);
    let aside = div()
        .flex()
        .flex_row()
        .gap_1()
        .child(widgets::count(changes.files as usize, "file"))
        .child(div().text_color(ok).child(format!("+{}", changes.added)))
        .child(
            div()
                .text_color(bad)
                .child(format!("\u{2212}{}", changes.removed)),
        );
    let mut body = div().flex().flex_col();
    for path in &changes.newest {
        body = body.child(row(
            mono(path.path.clone()),
            vec![
                div()
                    .meta()
                    .text_color(ok)
                    .child(format!("+{}", path.added)),
                div()
                    .meta()
                    .text_color(bad)
                    .child(format!("\u{2212}{}", path.removed)),
            ],
        ));
    }
    let more = (changes.files as usize).saturating_sub(changes.newest.len());
    if more > 0 {
        body = body.child(row(
            div()
                .small()
                .text_color(muted)
                .child(format!("{more} more")),
            Vec::new(),
        ));
    }
    Some(frame(form, "Changes", Some(aside), body, cx))
}

fn activity(overview: &Overview, form: Form, cx: &App) -> Option<Div> {
    let activity = &overview.activity;
    if activity.tool_runs == 0 && activity.agents_asked == 0 {
        return None;
    }
    let muted = cx.theme().muted_foreground;
    let aside = format!(
        "{} \u{00B7} {} asked",
        widgets::count(activity.tool_runs as usize, "tool run"),
        widgets::count(activity.agents_asked as usize, "agent"),
    );
    let mut body = div().flex().flex_col();
    for tool in &activity.by_tool {
        body = body.child(row(
            mono(tool.name.clone()),
            vec![div().meta().text_color(muted).child(tool.runs.to_string())],
        ));
    }
    Some(frame(form, "Activity", Some(div().child(aside)), body, cx))
}

fn log(shown: &Shown, form: Form, cx: &mut Context<Workspace>) -> Option<Div> {
    let newest = &shown.overview.as_ref()?.log;
    if newest.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let lines: Vec<&LogEntry> = match form {
        Form::Narrow => newest.iter().take(NARROW_LOG).collect(),
        Form::Wide => newest.iter().chain(&shown.older).collect(),
    };
    let mut body = div().flex().flex_col();
    for entry in lines {
        let (icon, color) = match entry.kind {
            LogKind::Started => (IconName::Play, theme.primary),
            LogKind::Done => (IconName::Check, theme.success),
            LogKind::Stopped => (IconName::Square, muted),
            LogKind::Failed => (IconName::Close, theme.danger),
        };
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .h(px(32.))
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .meta()
                        .text_color(muted)
                        .child(clock(entry.at)),
                )
                .child(Icon::from(icon).size(px(14.)).text_color(color))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .small()
                        .child(entry.text.to_string()),
                ),
        );
    }
    if form == Form::Wide && shown.more {
        body = body.child(
            div().pt_2().child(
                Button::new("overview-all-events")
                    .label("All events")
                    .on_click(cx.listener(|ws, _, _, cx| ws.load_older_events(cx))),
            ),
        );
    }
    let aside = div().child("newest first");
    Some(frame(form, "Event log", Some(aside), body, cx))
}

/// The time of day of a log line, in local time.
fn clock(at: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(at)
        .single()
        .map_or_else(String::new, |time| time.format("%H:%M").to_string())
}
