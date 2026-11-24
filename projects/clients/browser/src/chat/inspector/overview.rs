//! The Overview: how the chat's work stands, in sections that are absent when they have
//! nothing to say. Narrow in the column; "Open" widens the column and lays the same
//! sections out in two. Every number is the core's.

use crate::{
    api,
    link::Conn,
    state::state,
    ui::{Icon, IconName, compact, confirm, run, sleep, tone_text},
};
use dusk_protocol::thread::{
    ContextReport, EVENTS_PAGE, LogEntry, LogKind, MAX_TEAM_AT_ONCE, OVERVIEW_LOG, Overview,
    PlanCounts, Task, Team, ThreadTokens, plural,
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
    let form = AtOnceForm::new();
    // The plan's tasks and the token report as memos: the chat changes with every
    // streamed event, and only these parts of it are drawn here.
    let tasks = Memo::new(move |_| {
        app.chat(&id.get_value())
            .map(|chat| chat.with(|c| c.tasks.clone()))
            .unwrap_or_default()
    });

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
    let usage = Memo::new(move |_| {
        app.chat(&id.get_value())
            .and_then(|chat| chat.with(|data| data.usage))
    });
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
                        {progress(a.plan, a.team.as_ref(), id.get_value(), form)}
                        {a.team.as_ref().and_then(|team| {
                            tasks.with(|tasks| members(team, tasks, id.get_value()))
                        })}
                        {usage.get().and_then(|usage| tokens(usage, a.plan))}
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

/// The members-at-once form's state. It lives as long as the Overview, not the section
/// drawn around it, which the core's answers and the chat's events draw again and again.
#[derive(Clone, Copy)]
struct AtOnceForm {
    editing: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    at_once: RwSignal<u32>,
}

impl AtOnceForm {
    fn new() -> Self {
        Self {
            editing: RwSignal::new(false),
            error: RwSignal::new(None),
            at_once: RwSignal::new(1),
        }
    }
}

fn progress(
    counts: PlanCounts,
    team: Option<&Team>,
    thread: String,
    form: AtOnceForm,
) -> Option<AnyView> {
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
    let steer = team.map(|team| {
        // Only a running team can be stopped or changed.
        let controls = team
            .ended()
            .is_none()
            .then(|| view! { <TeamControls thread at_once=team.at_once form /> });
        // What it used: shown, never a limit.
        view! {
            <p class="ov-used">{team.used_line()}</p>
            {controls}
        }
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
                {steer}
            </section>
        }
        .into_any(),
    )
}

/// Stop team and members at once of a running team, under its Progress. Stopping asks
/// first; members at once open as a stepper in place, 1 to the protocol's most; the core
/// holds them to the settings and says so.
#[component]
fn TeamControls(thread: String, at_once: u32, form: AtOnceForm) -> impl IntoView {
    let thread = StoredValue::new(thread);
    let AtOnceForm {
        editing,
        error,
        at_once: asked,
    } = form;
    let stop = move |_| {
        let asked = "Stop the team? Members at work are stopped and their tasks go back to \
                     open. The plan and what is done stay.";
        if confirm(asked) {
            let id = thread.get_value();
            run(async move { api::stop_team(&id).await }, |()| ());
        }
    };
    let save = move |_| {
        let (id, at_once) = (thread.get_value(), asked.get_untracked());
        run(
            async move { api::set_team_at_once(&id, at_once).await },
            move |()| {
                editing.set(false);
                error.set(None);
            },
        );
    };
    let step = move |by: i32| {
        asked.update(|n| *n = n.saturating_add_signed(by).clamp(1, MAX_TEAM_AT_ONCE));
        error.set(None);
    };
    let form = move || {
        editing.get().then(|| {
            view! {
                <div class="ov-at-once">
                    <span class="muted">"Members at once"</span>
                    <button class="ghost" aria-label="Fewer"
                        prop:disabled=move || asked.get() <= 1
                        on:click=move |_| step(-1)>"\u{2212}"</button>
                    <b>{move || asked.get()}</b>
                    <button class="ghost" aria-label="More"
                        prop:disabled=move || asked.get() >= MAX_TEAM_AT_ONCE
                        on:click=move |_| step(1)>"+"</button>
                </div>
                {move || error.get().map(|why| view! { <p class="bad small">{why}</p> })}
                <div class="ov-actions">
                    <button class="ghost" on:click=move |_| editing.set(false)>"Cancel"</button>
                    <button class="primary" on:click=save>"Save"</button>
                </div>
            }
        })
    };
    view! {
        <Show when=move || !editing.get()>
            <div class="ov-actions">
                <button class="ghost" on:click=move |_| {
                    // The form opens on members at once as they are now.
                    asked.set(at_once);
                    error.set(None);
                    editing.set(true);
                }>"Members at once"</button>
                <button class="ghost danger" on:click=stop>"Stop team"</button>
            </div>
        </Show>
        {form}
    }
}

/// The team's members with their tasks, models and spend; a click opens a member's chat
/// as a subagent's opens.
fn members(team: &Team, tasks: &[Task], parent: String) -> Option<AnyView> {
    if team.members.is_empty() {
        return None;
    }
    let app = state();
    let rows = team
        .member_rows(tasks)
        .into_iter()
        .map(|member| {
            let class = tone_text(member.status.tone());
            let line = member.line();
            let open = (parent.clone(), member.thread_id.clone());
            view! {
                <button class="ov-member" on:click=move |_| app.subagent.set(Some(open.clone()))>
                    <span class=format!("ov-glyph {class}")><Icon name=IconName::Bot /></span>
                    <span class="ov-member-text">
                        <span class="name">{member.name}</span>
                        <span class="muted cut">{line}</span>
                    </span>
                    <span class=class>{member.status.member_word()}</span>
                </button>
            }
        })
        .collect_view();
    let aside = format!("{} of {} at work", team.at_work(), team.members.len());
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
