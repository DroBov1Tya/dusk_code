//! The Overview: how the chat's work stands, in sections that are absent when they have
//! nothing to say. Narrow in the column; "Open" widens the column and lays the same
//! sections out in two. Every number is the core's.

use crate::{
    api,
    link::Conn,
    state::state,
    ui::{Icon, IconName, compact, run, sleep},
};
use dusk_protocol::thread::{
    BudgetPart, ContextReport, EVENTS_PAGE, LogEntry, LogKind, OVERVIEW_LOG, Overview, PlanCounts,
    TaskStatus, Team, ThreadTokens, plural,
};
use leptos::prelude::*;

/// The Overview asks the core at most this often, in milliseconds.
const SPACING_MS: f64 = 1000.0;
/// The log lines the narrow form shows.
const NARROW_LOG: usize = 8;

#[derive(Clone, PartialEq)]
struct Answers {
    overview: Overview,
    plan: PlanCounts,
    team: Option<Team>,
    context: ContextReport,
}

#[component]
pub fn OverviewTab(id: String, wide: RwSignal<bool>) -> impl IntoView {
    let app = state();
    let id = StoredValue::new(id);
    let answers = RwSignal::new(None::<Answers>);
    let older = RwSignal::new(Vec::<LogEntry>::new());
    let more = RwSignal::new(false);
    let asked_at = StoredValue::new(f64::MIN);
    let waiting = StoredValue::new(false);

    let fetch = move || {
        asked_at.set_value(js_sys::Date::now());
        let thread = id.get_value();
        run(
            async move {
                Ok(Answers {
                    overview: api::overview(&thread).await?,
                    plan: api::plan_counts(&thread).await?,
                    team: api::team(&thread).await?,
                    context: api::context_report(Some(thread.clone())).await?,
                })
            },
            move |new: Answers| {
                if older.with_untracked(Vec::is_empty) {
                    more.set(new.overview.log.len() >= OVERVIEW_LOG);
                }
                answers.set(Some(new));
            },
        );
    };
    // At most once a second: a burst of events waits for the second and asks once.
    let refresh = move || {
        if waiting.get_value() {
            return;
        }
        let since = js_sys::Date::now() - asked_at.get_value();
        if since >= SPACING_MS {
            return fetch();
        }
        waiting.set_value(true);
        leptos::task::spawn_local(async move {
            sleep((SPACING_MS - since) as i32).await;
            waiting.set_value(false);
            fetch();
        });
    };
    // Asks when the tab opens, then after each turn's end and each change of the plan
    // or team.
    Effect::new(move |_| {
        let chat = app.chat(&id.get_value());
        chat.map(|chat| chat.with(|data| data.moved));
        refresh();
    });
    let usage = move || {
        app.chat(&id.get_value())
            .and_then(|chat| chat.with(|data| data.usage))
    };
    let load_older = move |_| {
        let thread = id.get_value();
        let oldest = older.with_untracked(|lines| lines.last().map(|line| line.at));
        let before = oldest.or_else(|| {
            answers.with_untracked(|a| {
                a.as_ref()
                    .and_then(|a| a.overview.log.last().map(|line| line.at))
            })
        });
        run(
            async move { api::events(&thread, before, EVENTS_PAGE).await },
            move |page| {
                more.set(page.len() == EVENTS_PAGE as usize);
                older.update(|lines| lines.extend(page));
            },
        );
    };
    let offline = move || app.conn.get() != Conn::Online;

    view! {
        <div class="overview" class:wide=move || wide.get() class:offline=offline>
            <div class="ov-bar">
                {move || offline().then(|| view! { <span class="muted">"Offline"</span> })}
                <span class="grow"></span>
                <button class="ghost" on:click=move |_| wide.update(|wide| *wide = !*wide)>
                    {move || if wide.get() { "Close" } else { "Open" }}
                </button>
            </div>
            {move || match answers.get() {
                None => view! { <p class="muted">"Reading the chat..."</p> }.into_any(),
                Some(a) => {
                    let narrow = !wide.get();
                    view! {
                        {progress(a.plan, a.team.as_ref())}
                        {a.team.as_ref().and_then(members)}
                        {usage().and_then(|usage| tokens(usage, a.plan))}
                        <section class="ov-sec">
                            <Head title="Context" aside=String::new() />
                            {super::context::report_view(a.context.clone())}
                        </section>
                        {changes(&a.overview)}
                        {activity(&a.overview)}
                        {log(&a.overview.log, older.get(), narrow)}
                        {(!narrow && more.get()).then(|| view! {
                            <button class="ghost ov-more" on:click=load_older>"All events"</button>
                        })}
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

#[component]
fn Head(title: &'static str, aside: String) -> impl IntoView {
    view! {
        <div class="ov-head"><b>{title}</b><span class="grow"></span><span class="muted">{aside}</span></div>
    }
}

fn figure(value: String, label: &'static str) -> impl IntoView {
    view! { <div class="ov-stat"><b>{value}</b><span>{label}</span></div> }
}

fn progress(counts: PlanCounts, team: Option<&Team>) -> Option<AnyView> {
    let running = counts.at_work();
    let all = counts.total();
    if all == 0 {
        return None;
    }
    let aside = team.map_or_else(String::new, Team::progress_note);
    let segments = ["done", "run", "block", ""]
        .into_iter()
        .zip(counts.shares())
        .filter(|(_, count)| *count > 0)
        // One segment a status, as wide as its share: 200 tasks stay one bar.
        .map(|(class, count)| view! { <i class=class style=format!("flex:{count}")></i> })
        .collect_view();
    let budget = team.map(|team| {
        team.budget_parts()
            .into_iter()
            .map(budget_row)
            .collect_view()
    });
    Some(
        view! {
            <section class="ov-sec">
                <Head title="Progress" aside=aside />
                <div class="ov-stats">
                    {figure(format!("{} of {all}", counts.done), "tasks done")}
                    {figure(running.to_string(), "running")}
                    {figure(counts.blocked.to_string(), "blocked")}
                </div>
                <div class="ov-segs" aria-hidden="true">{segments}</div>
                {budget}
            </section>
        }
        .into_any(),
    )
}

fn budget_row(part: BudgetPart) -> impl IntoView {
    view! {
        <div class="ov-budget">
            <span>{part.name}</span>
            <div class="ov-track">
                <i class:near=part.near() style=format!("width:{:.0}%", part.share() * 100.0)></i>
            </div>
            <span class="num">{part.text()}</span>
        </div>
    }
}

fn members(team: &Team) -> Option<AnyView> {
    if team.members.is_empty() {
        return None;
    }
    let at_work = team.at_work();
    let rows = team
        .members
        .iter()
        .map(|member| {
            let class = match member.status {
                TaskStatus::Open | TaskStatus::Claimed => "muted",
                TaskStatus::Running | TaskStatus::Review => "live",
                TaskStatus::Blocked => "warn",
                TaskStatus::Done => "good",
                TaskStatus::Failed => "bad",
            };
            let word = member.status.member_word();
            let role = member.role.clone().unwrap_or_else(|| "general".into());
            view! {
                <div class="ov-row">
                    <span class=format!("ov-glyph {class}")><Icon name=IconName::Bot /></span>
                    <span class="name">{member.name.clone()}</span>
                    <span class="muted cut grow">{role}</span>
                    <span class=class>{word}</span>
                </div>
            }
        })
        .collect_view();
    let aside = format!("{at_work} of {} at work", team.members.len());
    Some(
        view! { <section class="ov-sec"><Head title="Members" aside=aside />{rows}</section> }
            .into_any(),
    )
}

fn tokens(usage: ThreadTokens, plan: PlanCounts) -> Option<AnyView> {
    if usage.model_calls == 0 {
        return None;
    }
    let spent = usage.spent_tokens();
    let cached = usage.cached_percent();
    let calls = plural(usage.model_calls, "call");
    let aside = match usage.cost() {
        Some(cost) => format!("{calls} \u{00B7} {cost}"),
        None => calls,
    };
    let per_task = usage
        .per_task(plan.done)
        .map(|per_task| figure(compact(per_task), "per task done"));
    Some(
        view! {
            <section class="ov-sec">
                <Head title="Tokens" aside=aside />
                <div class="ov-stats">
                    {figure(compact(spent), "spent")}
                    {figure(format!("{cached}%"), "from cache")}
                    {per_task}
                </div>
            </section>
        }
        .into_any(),
    )
}

fn changes(overview: &Overview) -> Option<AnyView> {
    let changes = &overview.changes;
    if changes.files == 0 {
        return None;
    }
    let rows = changes
        .newest
        .iter()
        .map(|path| {
            view! {
                <div class="ov-row">
                    <span class="mono cut grow">{path.path.clone()}</span>
                    <span class="added">{format!("+{}", path.added)}</span>
                    <span class="removed">{format!("\u{2212}{}", path.removed)}</span>
                </div>
            }
        })
        .collect_view();
    let more = changes.more();
    let aside = changes.summary();
    Some(
        view! {
            <section class="ov-sec">
                <Head title="Changes" aside=aside />
                {rows}
                {(more > 0).then(|| view! { <div class="ov-row muted">{format!("{more} more")}</div> })}
            </section>
        }
        .into_any(),
    )
}

fn activity(overview: &Overview) -> Option<AnyView> {
    let activity = &overview.activity;
    if activity.tool_runs == 0 && activity.agents_asked == 0 {
        return None;
    }
    let aside = activity.summary();
    let rows = activity
        .by_tool
        .iter()
        .map(|tool| {
            view! {
                <div class="ov-row">
                    <span class="mono cut grow">{tool.name.clone()}</span>
                    <span class="muted">{tool.runs.to_string()}</span>
                </div>
            }
        })
        .collect_view();
    Some(
        view! { <section class="ov-sec"><Head title="Activity" aside=aside />{rows}</section> }
            .into_any(),
    )
}

fn log(newest: &[LogEntry], older: Vec<LogEntry>, narrow: bool) -> Option<AnyView> {
    if newest.is_empty() {
        return None;
    }
    let lines: Vec<LogEntry> = if narrow {
        newest.iter().take(NARROW_LOG).cloned().collect()
    } else {
        newest.iter().cloned().chain(older).collect()
    };
    let rows = lines
        .into_iter()
        .map(|line| {
            let (icon, class) = match line.kind {
                LogKind::Started => (IconName::Play, "live"),
                LogKind::Done => (IconName::Check, "good"),
                LogKind::Stopped => (IconName::Square, "muted"),
                LogKind::Failed => (IconName::Close, "bad"),
            };
            view! {
                <div class="ov-row">
                    <span class="ov-at muted">{clock(line.at)}</span>
                    <span class=format!("ov-glyph {class}")><Icon name=icon /></span>
                    <span class="cut grow">{line.text.to_string()}</span>
                </div>
            }
        })
        .collect_view();
    Some(
        view! {
            <section class="ov-sec ov-log">
                <Head title="Event log" aside="newest first".to_owned() />
                {rows}
            </section>
        }
        .into_any(),
    )
}

/// The local time of day of a log line.
fn clock(at: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(at as f64));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}
