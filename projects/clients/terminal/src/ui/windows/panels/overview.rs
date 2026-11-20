//! /overview: how the thread's work stands, as text, in the Overview's order: progress,
//! members, tokens, context, changes, activity and the event log. A section with
//! nothing to say is left out.

use ratatui::text::{Line, Span};

use dusk_protocol::thread::{LogKind, PlanCounts, Team, TeamEnd, TeamMember, ThreadTokens};

use super::context::context;
use crate::app::{App, OverviewView, compact_number};
use crate::ui::text::format::printable;
use crate::ui::windows::theme::{
    ACCENT, DONE, DOT, FAILED, NEGATIVE, ON, POSITIVE, TEXT, TEXT_MUTED, WARNING, bold, dim, fg,
    tone_fg,
};

pub fn overview(app: &App, view: &OverviewView) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(counts) = app.plan.filter(|counts| counts.total() > 0) {
        heading(&mut lines, "progress", plan_line(counts));
        if let Some(team) = &app.team {
            lines.push(status_line(team, "   "));
            let (budget, used) = (team.budget, team.used);
            lines.push(Line::from(Span::styled(
                format!(
                    "   steps {} of {} {DOT} tokens {} of {} {DOT} time {} of {} min",
                    used.steps,
                    budget.steps(),
                    compact_number(used.tokens),
                    compact_number(budget.tokens()),
                    used.minutes,
                    budget.minutes()
                ),
                dim(),
            )));
        }
    }
    if let Some(team) = app.team.as_ref().filter(|team| !team.members.is_empty()) {
        heading(&mut lines, "members", format!("{} at work", team.at_work()));
        for member in &team.members {
            lines.push(member_line(member, "   "));
        }
    }
    if app.tokens.model_calls > 0 {
        heading(&mut lines, "tokens", tokens_line(&app.tokens));
    }
    lines.push(Line::default());
    lines.extend(context(&view.context));
    let changes = &view.overview.changes;
    if changes.files > 0 {
        let sum = format!(
            "{} files {DOT} +{} \u{2212}{}",
            changes.files, changes.added, changes.removed
        );
        heading(&mut lines, "changes", sum);
        for path in &changes.newest {
            lines.push(Line::from(vec![
                Span::raw(format!("   {}", printable(&path.path))),
                Span::raw("\t"),
                Span::styled(format!("+{} \u{2212}{}  ", path.added, path.removed), dim()),
            ]));
        }
    }
    let activity = &view.overview.activity;
    if activity.tool_runs > 0 || activity.agents_asked > 0 {
        let sum = format!(
            "{} tool runs {DOT} {} agents asked",
            activity.tool_runs, activity.agents_asked
        );
        heading(&mut lines, "activity", sum);
        for tool in &activity.by_tool {
            lines.push(Line::from(vec![
                Span::raw(format!("   {}", printable(&tool.name))),
                Span::raw("\t"),
                Span::styled(format!("{}  ", tool.runs), dim()),
            ]));
        }
    }
    if !view.overview.log.is_empty() {
        heading(&mut lines, "event log", "newest first".into());
        for entry in &view.overview.log {
            let (glyph, color) = match entry.kind {
                LogKind::Started => (ON, ACCENT),
                LogKind::Done => (DONE, POSITIVE),
                LogKind::Stopped => (DOT, TEXT_MUTED),
                LogKind::Failed => (FAILED, NEGATIVE),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("   {}  ", clock(entry.at)), dim()),
                Span::styled(format!("{glyph} "), fg(color)),
                Span::raw(entry.text.to_string()),
            ]));
        }
    }
    lines
}

/// `plan 4/7 · ● 2 · ◆ 1`: done of all, at work, blocked.
pub fn plan_line(counts: PlanCounts) -> String {
    format!(
        "plan {}/{} {DOT} {ON} {} {DOT} \u{25C6} {}",
        counts.done,
        counts.total(),
        counts.at_work(),
        counts.blocked
    )
}

/// `running · 23 min`, or how the team ended and why, in a warning once it did.
pub fn status_line(team: &Team, indent: &str) -> Line<'static> {
    let color = match team.ended() {
        None => TEXT_MUTED,
        Some(TeamEnd::Done) => POSITIVE,
        Some(_) => WARNING,
    };
    Line::from(Span::styled(
        format!("{indent}{}", team.status_line()),
        fg(color),
    ))
}

/// `● Ember-0c coder running`, the dot in the colour of its task's state.
pub fn member_line(member: &TeamMember, indent: &str) -> Line<'static> {
    let (word, color) = (member.status.word(), tone_fg(member.status.tone()));
    let role = member.role.as_deref().unwrap_or("general");
    Line::from(vec![
        Span::styled(format!("{indent}{ON} "), fg(color)),
        Span::styled(printable(&member.name), bold(TEXT)),
        Span::styled(format!(" {}", printable(role)), dim()),
        Span::raw("\t"),
        Span::styled(format!("{word}  "), fg(color)),
    ])
}

fn tokens_line(tokens: &ThreadTokens) -> String {
    let spent = tokens.spent.prompt_tokens + tokens.spent.completion_tokens;
    let cached = tokens.spent.cached_prompt_tokens * 100 / tokens.spent.prompt_tokens.max(1);
    let mut line = format!(
        "{} spent {DOT} {cached}% from cache {DOT} {} calls",
        compact_number(spent),
        tokens.model_calls
    );
    if let Some(cost) = tokens.cost() {
        line.push_str(&format!(" {DOT} {cost}"));
    }
    line
}

fn heading(lines: &mut Vec<Line<'static>>, name: &str, value: String) {
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(format!(" {name}"), bold(TEXT)),
        Span::raw("\t"),
        Span::styled(format!("{value}  "), bold(TEXT)),
    ]));
}

fn clock(at: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(at)
        .single()
        .map_or_else(|| "--:--".into(), |time| time.format("%H:%M").to_string())
}
