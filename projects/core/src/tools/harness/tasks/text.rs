//! What the lead reads of the plan form: the description and schema it is offered,
//! and the plan as `show` prints it.

use dusk_protocol::{
    text::is_plain,
    thread::{GoalCheck, Team, TeamStatus},
};
use serde_json::{Value, json};

use crate::state::{PlanStatus, PlanTask, WaitsOn, short_task_id};

pub const DESCRIPTION: &str = "\
    The plan of this work, shared with the agents you lead; the core runs it. `add` \
    takes `tasks`: each has `text` and, as needed, `key` (your label for it in `after`), \
    `after` (keys or ids it depends on), `role` (the agent that does it; general when \
    left out), `files` (the paths only it may change), `accept` (how to tell it is \
    done), `gate` (a command that must exit 0; the core runs it) or `confirm` (the user \
    must confirm), `brief` (what the plan alone does not say), `phase`, `kind` (a \
    procedure). A cycle is refused. `run` starts the team with a `budget`: the core \
    gives each task whose `after` are done to an agent with only what that task needs, \
    checks gates, and writes to you only when a task waits for you, fails or is blocked, \
    and when the team ends. End your turn and wait; never ask for status. `review` \
    accepts `ids`, or rejects one `id` with `text` to redo. `show` prints the plan, or \
    one task whole with `id`. `edit` changes a task not yet started. `fail`, `reopen` \
    and `stop` do what they say. Use this instead of handing tasks out with ask_agent; \
    keep ask_agent for finding out what is unknown before you plan.";

/// The schema the lead is offered. Calls are checked against the union of both forms
/// (`checked_schema`) and parsed into `lead::PlanArguments`.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": [
                "add", "run", "review", "show", "edit", "fail", "reopen", "stop"]},
            "tasks": {"type": "array", "maxItems": 32, "items": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "key": {"type": "string"},
                    "after": {"type": "array", "items": {"type": "string"}},
                    "role": {"type": "string"},
                    "files": {"type": "array", "items": {"type": "string"}},
                    "accept": {"type": "string"},
                    "gate": {"type": "string"},
                    "confirm": {"type": "boolean"},
                    "brief": {"type": "string"},
                    "phase": {"type": "string"},
                    "kind": {"type": "string"}
                },
                "additionalProperties": false,
                "required": ["text"]
            }},
            "budget": {"type": "object", "properties": {
                "at_once": {"type": "integer", "minimum": 1},
                "steps": {"type": "integer", "minimum": 1},
                "tokens": {"type": "integer", "minimum": 1},
                "minutes": {"type": "integer", "minimum": 1}
            }, "additionalProperties": false},
            "id": {"type": "string"},
            "ids": {"type": "array", "items": {"type": "string"}},
            "text": {"type": "string"}
        },
        "additionalProperties": false,
        "required": ["action"]
    })
}

/// What a call is checked against: the board's schema with the plan's fields and
/// actions added, so a call of either form passes and its own parser decides.
pub fn checked_schema(board: Value) -> Value {
    let mut checked = board;
    let plan = schema();
    let Some(properties) = checked["properties"].as_object_mut() else {
        return checked;
    };
    for (name, field) in plan["properties"].as_object().into_iter().flatten() {
        properties
            .entry(name.clone())
            .or_insert_with(|| field.clone());
    }
    let Some(actions) = properties["action"]["enum"].as_array_mut() else {
        return checked;
    };
    for action in plan["properties"]["action"]["enum"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if !actions.contains(action) {
            actions.push(action.clone());
        }
    }
    checked
}

/// Lines `show` prints at most, and bytes.
pub const MAX_LINES: usize = 60;
pub const MAX_BYTES: usize = 4096;
/// Characters of a task's text on its line; `show` with an id prints it whole.
const LINE_TEXT: usize = 100;
/// Characters of a note on a line.
const LINE_NOTE: usize = 60;
/// Dependencies named on a line.
const LINE_AFTER: usize = 4;

/// Model or member text on one line: line breaks become spaces, other control characters
/// and the characters that hide or reorder text (`dusk_protocol::text::is_plain`) `?`, and
/// at most `max` characters, with `…` when cut. For display only;
/// what is stored is never cut.
pub fn plain(text: &str, max: usize) -> String {
    let mut out = String::new();
    for (count, c) in text.trim().chars().enumerate() {
        if count == max {
            out.push('…');
            break;
        }
        out.push(match c {
            '\n' | '\r' | '\t' => ' ',
            c if !is_plain(c) => '?',
            c => c,
        });
    }
    out
}

/// A task of the plan with the name of the member that holds it, if one does.
pub struct Shown<'a> {
    pub task: &'a PlanTask,
    pub member: Option<String>,
}

/// The plan as the lead reads it: a line of counts and the budget, then a line a task,
/// tasks not done first, at most `MAX_LINES` lines and `MAX_BYTES` bytes.
pub fn plan(tasks: &[Shown<'_>], team: Option<&Team>) -> String {
    if tasks.is_empty() {
        return "The plan is empty; add takes the whole plan in one call.".into();
    }
    let mut text = header(tasks, team);
    let order = tasks
        .iter()
        .filter(|shown| shown.task.status != PlanStatus::Done)
        .chain(
            tasks
                .iter()
                .filter(|shown| shown.task.status == PlanStatus::Done),
        );
    let mut written = 0;
    for shown in order {
        let line = line(shown);
        // Room is kept for the line that says how many were left out.
        if written == MAX_LINES || text.len() + line.len() + 1 + 64 > MAX_BYTES {
            break;
        }
        text.push('\n');
        text.push_str(&line);
        written += 1;
    }
    if written < tasks.len() {
        text.push_str(&format!(
            "\n… {} more not shown; show with an id prints one",
            tasks.len() - written
        ));
    }
    text
}

fn header(tasks: &[Shown<'_>], team: Option<&Team>) -> String {
    let count = |statuses: &[PlanStatus]| {
        tasks
            .iter()
            .filter(|shown| statuses.contains(&shown.task.status))
            .count()
    };
    let mut text = format!(
        "Plan: {} of {} done",
        count(&[PlanStatus::Done]),
        tasks.len()
    );
    for (statuses, word) in [
        (&[PlanStatus::Claimed, PlanStatus::Running][..], "running"),
        (&[PlanStatus::Review][..], "in review"),
        (&[PlanStatus::Blocked][..], "blocked"),
        (&[PlanStatus::Failed][..], "failed"),
        (&[PlanStatus::Open][..], "open"),
    ] {
        let n = count(statuses);
        if n > 0 {
            text.push_str(&format!(", {n} {word}"));
        }
    }
    text.push('.');
    if let Some(team) = team.filter(|team| team.status == TeamStatus::Running) {
        let (budget, used) = (team.budget, team.used);
        text.push_str(&format!(
            " Budget: {}/{} steps, {}/{} tokens, {}/{} min.",
            used.steps,
            budget.steps(),
            amount(used.tokens),
            amount(budget.tokens()),
            used.minutes,
            budget.minutes()
        ));
    }
    text
}

/// 1310000 as `1.31M`, 4000000 as `4M`, 2500 as `2.5k`.
fn amount(n: u64) -> String {
    let (value, unit) = match n {
        1_000_000.. => (n as f64 / 1_000_000.0, "M"),
        1_000.. => (n as f64 / 1_000.0, "k"),
        _ => return n.to_string(),
    };
    let digits = format!("{value:.2}");
    let digits = digits.trim_end_matches('0').trim_end_matches('.');
    format!("{digits}{unit}")
}

fn mark(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::Open => "[ ]",
        PlanStatus::Claimed | PlanStatus::Running => "[>]",
        PlanStatus::Review => "[?]",
        PlanStatus::Blocked => "[!]",
        PlanStatus::Done => "[x]",
        PlanStatus::Failed => "[-]",
    }
}

fn role(task: &PlanTask) -> String {
    task.role
        .as_deref()
        .map_or_else(|| "general".to_owned(), |role| plain(role, 64))
}

/// `[>] #77aa01bc make it so (general, Ember-0c) after 5d01aa3c`.
fn line(shown: &Shown<'_>) -> String {
    let task = shown.task;
    let mut text = format!(
        "{} #{} {} ({}",
        mark(task.status),
        short_task_id(&task.task_id),
        plain(&task.description, LINE_TEXT),
        role(task)
    );
    if task.status != PlanStatus::Done
        && let Some(member) = &shown.member
    {
        text.push_str(&format!(", {}", plain(member, 32)));
    }
    text.push(')');
    text.push_str(&state_words(task));
    text
}

/// What the lead may need to know of a task beyond its mark.
fn state_words(task: &PlanTask) -> String {
    let note = || plain(&task.note, LINE_NOTE);
    match task.status {
        PlanStatus::Review => match (&task.waits_on, &task.gate) {
            (Some(WaitsOn::Lead), _) | (_, None) => " waits for you".into(),
            (_, Some(GoalCheck::UserConfirmation)) => " waits for the user".into(),
            (_, Some(GoalCheck::Command { .. })) => " gate runs".into(),
        },
        PlanStatus::Blocked => match &task.waits_on {
            Some(WaitsOn::Task(id)) => format!(" behind failed {}", short_task_id(id)),
            Some(WaitsOn::Message(_)) => " waits on a message".into(),
            _ if task.note.is_empty() => String::new(),
            _ => format!(": {}", note()),
        },
        PlanStatus::Failed if !task.note.is_empty() => format!(": {}", note()),
        PlanStatus::Open | PlanStatus::Claimed | PlanStatus::Running
            if !task.depends_on.is_empty() =>
        {
            let after: Vec<&str> = task
                .depends_on
                .iter()
                .take(LINE_AFTER)
                .map(|id| short_task_id(id))
                .collect();
            let more = task.depends_on.len().saturating_sub(LINE_AFTER);
            let more = if more > 0 {
                format!(" +{more}")
            } else {
                String::new()
            };
            format!(" after {}{more}", after.join(" "))
        }
        _ => String::new(),
    }
}

/// One task whole, for `show` with an id. `after` holds each dependency's status.
pub fn task(shown: &Shown<'_>, after: &[(String, Option<PlanStatus>)]) -> String {
    let task = shown.task;
    let mut text = format!(
        "#{} {}: {}\nRole: {}",
        short_task_id(&task.task_id),
        task.status.as_str(),
        plain(&task.description, usize::MAX),
        role(task)
    );
    if let Some(member) = &shown.member {
        text.push_str(&format!(", {}", plain(member, 32)));
    }
    if !task.phase.is_empty() {
        text.push_str(&format!(". Phase: {}", plain(&task.phase, usize::MAX)));
    }
    text.push_str(&format!(
        ". Attempts: {}, reworks: {}.",
        task.attempts, task.reworks
    ));
    if !after.is_empty() {
        let after: Vec<String> = after
            .iter()
            .map(|(id, status)| {
                let status = status.map_or("gone", PlanStatus::as_str);
                format!("#{} ({status})", short_task_id(id))
            })
            .collect();
        text.push_str(&format!("\nAfter: {}", after.join(", ")));
    }
    if !task.files.entries().is_empty() {
        text.push_str(&format!("\nFiles: {}", task.files.entries().join(", ")));
    }
    let gate = match &task.gate {
        Some(GoalCheck::Command { command }) => format!("`{}` exits 0", plain(command, 400)),
        Some(GoalCheck::UserConfirmation) => "the user confirms".into(),
        None => "you review".into(),
    };
    text.push_str(&format!("\nGate: {gate}"));
    for (label, field) in [
        ("Accept", &task.acceptance),
        ("Brief", &task.brief),
        ("Note", &task.note),
    ] {
        if !field.is_empty() {
            text.push_str(&format!("\n{label}: {}", plain(field, usize::MAX)));
        }
    }
    text.push_str(&format!("\nId: {}", task.task_id));
    text
}
