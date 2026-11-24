//! Made-up chats for looking at the page without a core, in debug builds only:
//! `?fake=team|team-ended|subagents|subagents-ended` opens a chat that set a team of
//! seven to work on a plan of eight tasks, one in each status, or asked three subagents in
//! one step, as the core tells it, running or ended. Nothing is sent anywhere.

use crate::{
    chat::data::{ChatData, now_ms},
    link::Conn,
    state::{AppState, View},
};
use dusk_protocol::{
    model::{ModelEvent, ToolCall},
    thread::{
        Message, MessageRole, Task, TaskStatus, Team, TeamBudget, TeamEnd, TeamMember, TeamStatus,
        TeamUsed, ThreadEvent, Turn, TurnStatus, agent_name, team_call_id,
    },
    tools::ToolResult,
};
use leptos::prelude::*;

const CHAT: &str = "fake";

/// The fake the address asks for.
pub fn asked() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("fake="))
        .map(str::to_owned)
}

/// Opens the made-up chat `fake` names.
pub fn fill(app: AppState, fake: &str) {
    let mut chat = ChatData::default();
    let ended = fake.ends_with("-ended");
    let (asked, answer) = match fake.starts_with("subagents") {
        true => (
            "Look at the cache from three sides.",
            "Asking three subagents.",
        ),
        false => (
            "Make the cache drop expired entries.",
            "The plan is ready; setting the team to work.",
        ),
    };
    let now = now_ms();
    chat.title = Some("Drop expired entries from the cache".into());
    chat.apply(ThreadEvent::UserMessage(user(asked, now)));
    chat.apply(ThreadEvent::TurnStarted(turn(TurnStatus::Running, now)));
    chat.apply(ThreadEvent::Model(ModelEvent::Text(answer.into())));
    match fake.starts_with("subagents") {
        true => subagents(&mut chat, ended),
        false => team(&mut chat, ended, now),
    }
    if ended {
        chat.apply(ThreadEvent::TurnFinished(turn(TurnStatus::Completed, now)));
    }
    app.chats
        .update(|chats| _ = chats.insert(CHAT.into(), RwSignal::new(chat)));
    app.conn.set(Conn::Online);
    // With no core every call fails; its toast would only cover the page.
    Effect::new(move |_| {
        if app.toast.with(Option::is_some) {
            app.toast.set(None);
        }
    });
    app.view.set(View::Chat(Some(CHAT.into())));
}

fn user(content: &str, now: i64) -> Message {
    Message {
        message_id: "fake-1".into(),
        thread_id: CHAT.into(),
        sequence: 1,
        role: MessageRole::User,
        content: content.into(),
        reasoning: None,
        tool_calls: Vec::new(),
        tool_call_id: None,
        artifacts: Vec::new(),
        from_thread_id: None,
        from_job_id: None,
        content_tokens: 0,
        tool_calls_tokens: 0,
        reasoning_tokens: 0,
        created_at: now,
    }
}

fn turn(status: TurnStatus, now: i64) -> Turn {
    Turn {
        turn_id: "fake-turn".into(),
        thread_id: CHAT.into(),
        last_sequence: 1,
        response_message_id: None,
        status,
        started_at: now,
        finished_at: (status != TurnStatus::Running).then_some(now),
        error_message: None,
        waiting: None,
        steps: 3,
        tokens: 0,
        cost_micro_usd: 0,
    }
}

fn call(id: &str, name: &str, arguments: serde_json::Value) -> ThreadEvent {
    ThreadEvent::ToolStarted(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    })
}

fn finished(id: &str, text: &str) -> ThreadEvent {
    ThreadEvent::ToolFinished {
        call_id: id.into(),
        result: ToolResult::text(text.into()),
    }
}

/// The plan's tasks: number, words, status and role; the ended team gave back those at
/// work, so they are open and owned by no one.
fn plan(ended: bool) -> Vec<(u32, &'static str, TaskStatus, &'static str)> {
    use TaskStatus::*;
    let plan = [
        (1, "Plan the cache layout", Done, "architect"),
        (2, "Give Entry a deadline", Done, "backend-engineer"),
        (
            3,
            "Remove expired entries in get()",
            Running,
            "backend-engineer",
        ),
        (4, "Write tests for expiry", Blocked, "qa-engineer"),
        (5, "Check the cache key handling", Failed, "appsec-reviewer"),
        (
            6,
            "Review the diff of src/cache.rs",
            Review,
            "code-reviewer",
        ),
        (7, "Measure memory growth", Claimed, "sre-engineer"),
        (8, "Write what changed", Open, "docs"),
    ];
    plan.into_iter()
        .map(
            |(number, text, status, role)| match ended && status.at_work() {
                true => (number, text, Open, role),
                false => (number, text, status, role),
            },
        )
        .collect()
}

fn member(number: u32) -> String {
    format!(
        "0190f1c2-7a3b-7c4d-8e5f-{:012x}",
        u64::from(number) * 0x9e37_79b9
    )
}

fn team(chat: &mut ChatData, ended: bool, now: i64) {
    chat.apply(call(
        "call-run",
        "tasks",
        serde_json::json!({ "action": "run" }),
    ));
    chat.apply(finished("call-run", "The team started."));
    let plan = plan(ended);
    let owned = |status: TaskStatus| status != TaskStatus::Open;
    for &(number, text, status, role) in &plan {
        let mut task = serde_json::json!({
            "task_id": format!("t{number}"), "description": text,
            "done": status == TaskStatus::Done, "status": status.name(),
            "created_at": now - 3_400_000, "updated_at": now, "items": [], "role": role,
            "attempts": 1,
        });
        if status == TaskStatus::Review {
            task["gate"] = serde_json::json!({ "kind": "user_confirmation" });
        }
        if owned(status) {
            let thread_id = member(number);
            task["owner"] =
                serde_json::json!({ "name": agent_name(&thread_id), "thread_id": thread_id });
        }
        match serde_json::from_value::<Task>(task) {
            Ok(task) => _ = chat.apply(ThreadEvent::TaskChanged(task)),
            Err(error) => leptos::logging::error!("a made-up task does not read: {error}"),
        }
        if owned(status) {
            chat.apply(ThreadEvent::SubagentStarted {
                call_id: team_call_id(&format!("t{number}")),
                thread_id: member(number),
                agent: role.into(),
                task: text.into(),
                effort: None,
                model: Some("chatgpt/gpt-5.5".into()),
                task_id: Some(format!("t{number}")),
            });
        }
    }
    let members = plan
        .iter()
        .filter(|(.., status, _)| owned(*status))
        .map(|&(number, _, status, role)| TeamMember {
            thread_id: member(number),
            name: agent_name(&member(number)),
            role: Some(role.into()),
            model: Some("chatgpt/gpt-5.5".into()),
            task_id: format!("t{number}"),
            status,
        })
        .collect();
    let Ok(budget) = TeamBudget::new(3, 400, 4_000_000, 120) else {
        return;
    };
    chat.apply(ThreadEvent::Team(Team {
        thread_id: CHAT.into(),
        status: match ended {
            true => TeamStatus::Ended(TeamEnd::Steps),
            false => TeamStatus::Running,
        },
        budget,
        used: TeamUsed {
            at_once: if ended { 0 } else { 2 },
            steps: 182,
            tokens: 1_240_000,
            minutes: 1,
        },
        members,
        // Started 72 seconds ago, as the card the owner drew: "1m 12s".
        started_at: Some(now - 72_000),
        ended_at: ended.then_some(now - 5_000),
    }));
}

/// Three subagents asked in one step 40 seconds ago: one done, one at work (done when
/// ended) and one failed.
fn subagents(chat: &mut ChatData, ended: bool) {
    let asked = [
        ("Read the cache module", ThreadEvent::Idle),
        (
            "Measure memory growth",
            match ended {
                true => ThreadEvent::Idle,
                false => ThreadEvent::Model(ModelEvent::Text("Measuring".into())),
            },
        ),
        (
            "Check the cache key handling",
            ThreadEvent::Failed("the provider is unavailable".into()),
        ),
    ];
    for (index, (task, _)) in asked.iter().enumerate() {
        chat.apply(call(
            &format!("ask-{index}"),
            "ask_agent",
            serde_json::json!({ "task": task }),
        ));
    }
    for (index, (task, last)) in asked.into_iter().enumerate() {
        let thread_id = format!("0000010{index}-fake");
        chat.apply(ThreadEvent::SubagentStarted {
            call_id: format!("ask-{index}"),
            thread_id: thread_id.clone(),
            agent: "general".into(),
            task: task.into(),
            effort: None,
            model: Some("chatgpt/gpt-5.5".into()),
            task_id: None,
        });
        chat.apply(finished(&format!("ask-{index}"), "sent"));
        let started = ThreadEvent::Model(ModelEvent::Text("Reading the task.".into()));
        for event in [started, last] {
            chat.apply(ThreadEvent::Subagent {
                thread_id: thread_id.clone(),
                event: Box::new(event),
            });
        }
        if let Some(agent) = chat.agents.get_mut(&thread_id) {
            agent.started_at -= 40_000;
            if let Some(ended_at) = agent.ended_at.as_mut() {
                *ended_at -= 1_000;
            }
        }
    }
}
