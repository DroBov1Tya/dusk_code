//! What the lead reads of the plan form: the description and schema it is offered,
//! and the plan as `show` prints it.

use dusk_protocol::{
    text::is_plain,
    thread::{GoalCheck, TaskDuty, Team, TeamStatus},
};
use serde_json::{Value, json};

use crate::state::{PlanStatus, PlanTask, WaitsOn, short_task_id};

pub const DESCRIPTION: &str = "\
    The plan of this work, shared with the agents you lead; the core runs it, and it is \
    the only way work is handed out. `add` takes `tasks`: each has `text`, `accept` (how \
    to tell it is done) and `files` (the paths only it may change; a scout task, role \
    `scout`, has none and changes nothing), and as needed `key` (your label for it in \
    `after`), `after` (keys or ids it depends on), `role` (the agent that does it; \
    general when left out), `gate` (a command that must exit 0; the core runs it) or \
    `confirm` (the user must confirm), `brief` (to a peer: goal, ruled out, files and \
    why, how to check), `phase`, `tier` (its model: strong to design, review or debug, \
    weak for mechanical or read-only work) and `effort`. Tasks of one add that share a file need an `after` \
    between them; a cycle is refused. `run` takes `goal` (one sentence and how done is \
    told) and starts the team: the core gives out one wave at a time (a wave starts \
    when the one before is all done or skipped), runs gates, has every change reviewed \
    by a fresh reviewer, and writes to you only when a task waits for you, fails or is \
    blocked, and when the team ends. End your turn and wait; never ask for status. \
    `review` accepts `ids`, or rejects one `id` with `text` to redo. `show` prints the \
    plan, or one task whole with its report with `id`. `edit` changes a task not yet \
    started. `skip` gives up a failed or blocked task (with `text`, why) so the next \
    wave can start. `fail`, `reopen` and `stop` do what they say.";

/// The schema the lead is offered. Calls are checked against the union of both forms
/// (`checked_schema`) and parsed into `lead::PlanArguments`.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": [
                "add", "run", "review", "show", "edit", "fail", "reopen", "skip", "stop"]},
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
                    "kind": {"type": "string"},
                    "tier": {"type": "string", "enum": ["strong", "default", "weak"]},
                    "effort": {"type": "string", "enum": [
                        "off", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]}
                },
                "additionalProperties": false,
                "required": ["text"]
            }},
            "id": {"type": "string"},
            "ids": {"type": "array", "items": {"type": "string"}},
            "text": {"type": "string"},
            "goal": {"type": "string"}
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

/// The plan as the lead reads it: a line of counts and members at work, then a line a task,
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
    let skipped = tasks
        .iter()
        .filter(|shown| shown.task.skipped && shown.task.status != PlanStatus::Done)
        .count();
    if skipped > 0 {
        text.push_str(&format!(" ({skipped} of them skipped)"));
    }
    text.push('.');
    if let Some(team) = team.filter(|team| team.status == TeamStatus::Running) {
        text.push_str(&format!(
            " At work: {} of {}.",
            team.used.at_once, team.at_once
        ));
    }
    text
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
    let mut words = match (task.duty, &task.review_of) {
        (TaskDuty::Review, Some(of)) => format!(" review of {}", short_task_id(of)),
        (TaskDuty::Critic, _) => " completeness check".to_owned(),
        _ => String::new(),
    };
    if task.skipped && task.status != PlanStatus::Done {
        words.push_str(" skipped");
    }
    words.push_str(&status_words(task));
    words
}

fn status_words(task: &PlanTask) -> String {
    let note = || plain(&task.note, LINE_NOTE);
    match task.status {
        PlanStatus::Review => match (&task.waits_on, &task.gate) {
            (Some(WaitsOn::Review(row)), _) => format!(" reviewed by {}", short_task_id(row)),
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
    if task.skipped {
        text.push_str("\nSkipped: the next wave does not wait for it");
    }
    if let Some(of) = &task.review_of {
        text.push_str(&format!("\nReviews: #{}", short_task_id(of)));
    }
    // The member's or the checker's own words: data, each line made plain.
    if !task.result.is_empty() {
        let lines: Vec<String> = task
            .result
            .lines()
            .map(|line| plain(line, usize::MAX))
            .collect();
        text.push_str(&format!(
            "\nReport (the member's words):\n{}",
            lines.join("\n")
        ));
    }
    text.push_str(&format!("\nId: {}", task.task_id));
    text
}
