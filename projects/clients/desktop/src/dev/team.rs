//! `fake=team|team-ended`: after `fake=chat`, the shown chat leads a team of members on a
//! plan of eight tasks, one in each status, with two fresh reviews (one approved, one
//! asking for a fix), a skipped task and the critic still to come, as the core tells it: a `Team`
//! event, `TaskChanged` for every task and the members' own events. `team-ended` is the
//! same team after it ran out of steps. The Overview and the plan strip get the answers
//! their requests would give. Made-up data only.

use dusk_protocol::{
    model::{ModelEvent, ToolCall},
    system::now_ms,
    thread::{
        LogEntry, LogKind, Overview, PlanCounts, Task, TaskDuty, TaskStatus, Team, TeamEnd,
        TeamMember, TeamStatus, TeamUsed, ThreadEvent, agent_name,
    },
};
use gpui_kit::*;

use crate::workspace::Workspace;

/// A task of the made-up plan: its number, words, status, role and what else it holds.
struct Made {
    number: u32,
    text: &'static str,
    status: TaskStatus,
    role: &'static str,
    owned: bool,
    extra: serde_json::Value,
}

fn plan() -> Vec<Made> {
    use serde_json::json;
    let made = |number, text, status, role, owned, extra| Made {
        number,
        text,
        status,
        role,
        owned,
        extra,
    };
    vec![
        made(
            1,
            "Plan the cache layout",
            TaskStatus::Done,
            "architect",
            true,
            json!({ "acceptance": "A short note in docs/cache.md", "files": ["docs/cache.md"],
                    "steps": 14, "tokens": 61_000, "changes": 1 }),
        ),
        made(
            2,
            "Give Entry a deadline",
            TaskStatus::Done,
            "backend-engineer",
            true,
            json!({ "depends_on": ["t1"], "files": ["src/entry.rs"],
                    "gate": { "kind": "command", "command": "cargo test -p cache" },
                    "result": "Status: done\nResult: Entry carries a deadline (src/entry.rs)\n\
                               Checked: cargo test -p cache, 14 passed\n\
                               Not checked: the sweep under load\nOpen: nothing",
                    "steps": 22, "tokens": 118_000, "changes": 2 }),
        ),
        made(
            3,
            "Remove expired entries in get()",
            TaskStatus::Running,
            "backend-engineer",
            true,
            json!({ "depends_on": ["t2"], "files": ["src/cache.rs"],
                    "brief": "get() compares the deadline with the clock and removes an entry past it.",
                    "acceptance": "Reading an expired key returns None and frees it",
                    "gate": { "kind": "command", "command": "cargo test -p cache" },
                    "attempts": 1, "reworks": 1,
                    "note": "review found 1 problem; show #t10",
                    "steps": 31, "tokens": 204_000, "changes": 1 }),
        ),
        made(
            4,
            "Write tests for expiry",
            TaskStatus::Blocked,
            "qa-engineer",
            true,
            json!({ "depends_on": ["t2"], "files": ["tests/expiry.rs"],
                    "acceptance": "Three tests: on read, by the sweep, and a live key kept",
                    "waits_on": "lead", "note": "which clock to mock: Instant or a fake clock?",
                    "steps": 12, "tokens": 84_000 }),
        ),
        made(
            5,
            "Check the cache key handling",
            TaskStatus::Failed,
            "appsec-reviewer",
            true,
            json!({ "files": ["src/key.rs"], "attempts": 3, "reworks": 2, "skipped": true,
                    "note": "gave up after two reworks: the key is hashed with the default hasher",
                    "steps": 40, "tokens": 310_000 }),
        ),
        made(
            6,
            "Review the diff of src/cache.rs",
            TaskStatus::Review,
            "code-reviewer",
            true,
            json!({ "depends_on": ["t2"], "gate": { "kind": "user_confirmation" },
                    "steps": 9, "tokens": 52_000 }),
        ),
        made(
            7,
            "Measure memory growth",
            TaskStatus::Claimed,
            "sre-engineer",
            true,
            json!({ "depends_on": ["t3"], "steps": 0, "tokens": 0 }),
        ),
        made(
            8,
            "Write what changed",
            TaskStatus::Open,
            "docs",
            false,
            json!({ "depends_on": ["t6", "t7"], "files": ["CHANGELOG.md"] }),
        ),
        // The core's own rows: a fresh review of each task that changed files, and the
        // completeness check after the last wave.
        made(
            9,
            "Review 1 of #t2: Give Entry a deadline",
            TaskStatus::Done,
            "code-reviewer",
            true,
            json!({ "depends_on": ["t1"], "duty": "review", "review_of": "t2",
                    "result": "Verdict: approve\nChecked: read src/entry.rs, ran cargo test\n\
                               Not checked: nothing", "steps": 6, "tokens": 40_000 }),
        ),
        made(
            10,
            "Review 1 of #t3: Remove expired entries in get()",
            TaskStatus::Done,
            "code-reviewer",
            true,
            json!({ "depends_on": ["t2"], "duty": "review", "review_of": "t3",
                    "result": "Problem: high | src/cache.rs:88 | an entry past its deadline \
                               is returned once | compare before the read\nVerdict: fix\n\
                               Checked: cargo test -p cache\nNot checked: nothing",
                    "steps": 8, "tokens": 51_000 }),
        ),
        made(
            11,
            "Completeness check",
            TaskStatus::Open,
            "code-reviewer",
            false,
            json!({ "duty": "critic" }),
        ),
    ]
}

/// The thread of the member that holds task `number`; its name comes from it.
fn member_thread(number: u32) -> String {
    format!(
        "0190f1c2-7a3b-7c4d-8e5f-{:012x}",
        u64::from(number) * 0x9e37_79b9
    )
}

fn task(made: &Made, now: i64) -> Task {
    let mut json = serde_json::json!({
        "task_id": format!("t{}", made.number), "description": made.text,
        "done": made.status == TaskStatus::Done, "status": made.status.name(),
        "created_at": now - 3_400_000, "updated_at": now - 60_000 * i64::from(made.number),
        "items": [], "role": made.role, "phase": "expiry", "attempts": u32::from(made.owned),
        "model": made.owned.then_some("chatgpt/gpt-5.5"),
    });
    if made.owned {
        let thread_id = member_thread(made.number);
        json["owner"] =
            serde_json::json!({ "name": agent_name(&thread_id), "thread_id": thread_id });
    }
    if let (Some(json), Some(extra)) = (json.as_object_mut(), made.extra.as_object()) {
        json.extend(extra.clone());
    }
    serde_json::from_value(json).expect("a made-up task reads back")
}

fn team(lead: &str, ended: bool, plan: &[Made]) -> Team {
    let members = plan
        .iter()
        .filter(|made| made.owned)
        .rev()
        .map(|made| {
            let thread_id = member_thread(made.number);
            TeamMember {
                name: agent_name(&thread_id),
                thread_id,
                role: Some(made.role.into()),
                model: Some("chatgpt/gpt-5.5".into()),
                task_id: format!("t{}", made.number),
                status: made.status,
            }
        })
        .collect();
    let (status, used) = match ended {
        false => (
            TeamStatus::Running,
            TeamUsed {
                at_once: 2,
                steps: 182,
                tokens: 1_240_000,
                minutes: 23,
            },
        ),
        true => (
            TeamStatus::Ended(TeamEnd::Stalled),
            TeamUsed {
                at_once: 0,
                steps: 400,
                tokens: 2_310_000,
                minutes: 57,
            },
        ),
    };
    Team {
        thread_id: lead.into(),
        status,
        at_once: 3,
        used,
        members,
        started_at: Some(now_ms() - 23 * 60_000),
        goal: "Expired cache entries are gone on read and by the sweep; cargo test -p cache \
               passes"
            .into(),
    }
}

/// What each member does now, as its own events say.
fn member_events(made: &Made) -> Vec<ThreadEvent> {
    let tool = |name: &str, arguments: serde_json::Value| {
        ThreadEvent::ToolStarted(ToolCall {
            id: format!("call-{name}"),
            name: name.into(),
            arguments,
        })
    };
    let started = ThreadEvent::Model(ModelEvent::Text("Reading the task.".into()));
    match made.status {
        TaskStatus::Running => vec![
            started,
            tool(
                "run_command",
                serde_json::json!({ "command": "cargo test -p cache" }),
            ),
        ],
        TaskStatus::Claimed => vec![started],
        TaskStatus::Failed => vec![ThreadEvent::Failed("the gate failed twice".into())],
        _ => vec![started, ThreadEvent::Idle],
    }
}

pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let ended = value == "team-ended";
    let now = now_ms();
    let Some(lead) = ws.shown.clone() else {
        eprintln!("fake=team needs a shown chat: fake=chat first");
        return;
    };
    let mut made = plan();
    // The end gave the tasks at work back: they are open and owned by no one, so their
    // members drop off the roster, as the core sends it.
    if ended {
        for made in &mut made {
            if made.status.at_work() {
                made.status = TaskStatus::Open;
                made.owned = false;
            }
        }
    }
    let team = team(&lead, ended, &made);
    let tasks: Vec<Task> = made.iter().map(|made| task(made, now)).collect();
    if let Some(chat) = ws.chats.get_mut(&lead) {
        chat.tasks.clear();
        for task in &tasks {
            chat.apply(ThreadEvent::TaskChanged(task.clone()));
        }
        for made in made.iter().filter(|made| made.owned) {
            // The core announces each member as it gives it its task.
            let task_id = format!("t{}", made.number);
            chat.apply(ThreadEvent::SubagentStarted {
                call_id: dusk_protocol::thread::team_call_id(&task_id),
                thread_id: member_thread(made.number),
                agent: made.role.into(),
                task: made.text.into(),
                effort: None,
                model: Some("chatgpt/gpt-5.5".into()),
                task_id: Some(task_id),
            });
            for event in member_events(made) {
                chat.apply(ThreadEvent::Subagent {
                    thread_id: member_thread(made.number),
                    event: Box::new(event),
                });
            }
        }
        chat.apply(ThreadEvent::Team(team.clone()));
    }
    // The core counts the lead's work only.
    let mut counts = PlanCounts::default();
    for task in tasks.iter().filter(|task| task.duty == TaskDuty::Work) {
        let count = match task.status {
            TaskStatus::Open => &mut counts.open,
            TaskStatus::Claimed => &mut counts.claimed,
            TaskStatus::Running => &mut counts.running,
            TaskStatus::Blocked => &mut counts.blocked,
            TaskStatus::Review => &mut counts.review,
            TaskStatus::Done => &mut counts.done,
            TaskStatus::Failed => &mut counts.failed,
        };
        *count += 1;
    }
    let line = |minutes: i64, kind, text: &str| LogEntry {
        at: now - minutes * 60_000,
        kind,
        text: dusk_protocol::thread::Line::new(text).expect("a short log line"),
    };
    let mut log = vec![
        line(
            9,
            LogKind::Failed,
            "Task #5 failed: Check the cache key handling",
        ),
        line(21, LogKind::Done, "Task #2 done: Give Entry a deadline"),
        line(23, LogKind::Started, "Team started"),
    ];
    if ended {
        log.insert(
            0,
            line(0, LogKind::Stopped, "Team ended (steps) after 57m 0s"),
        );
    }
    ws.dock.counts = Some((lead.clone(), counts));
    ws.overview.chat = Some(lead);
    ws.overview.overview = Some(Overview {
        log,
        ..Overview::default()
    });
    ws.overview.plan = Some(counts);
    ws.overview.team = Some(team);
    cx.notify();
}
