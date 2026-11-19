use super::*;
use dusk_protocol::{
    catalog::SkillSave,
    model::{ModelEvent, ToolCall},
    thread::{Block, MessageRole},
    tools::ToolResult,
};
use serde_json::{Value, json};

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}

fn flow() -> Value {
    json!({ "flow": { "layers": [
        { "title": "Parse", "cards": [{ "title": "lexer", "line": "tokens" }] },
        { "title": "Check", "emphasis": true },
    ] } })
}

/// What `show` answers: the block's text, or the refusal.
fn result(call: &ToolCall) -> ToolResult {
    let content = Block::asked(call).map_or("error: refused".into(), |block| block.text());
    ToolResult {
        is_error: content.starts_with("error:"),
        content,
        exit_code: None,
        artifacts: Vec::new(),
    }
}

fn message(
    role: MessageRole,
    content: &str,
    calls: Vec<ToolCall>,
    answers: Option<&str>,
) -> Message {
    Message {
        message_id: String::new(),
        thread_id: String::new(),
        sequence: 0,
        role,
        content: content.into(),
        reasoning: None,
        tool_calls: calls,
        tool_call_id: answers.map(Into::into),
        artifacts: Vec::new(),
        from_thread_id: None,
        from_job_id: None,
        content_tokens: 0,
        tool_calls_tokens: 0,
        reasoning_tokens: 0,
        created_at: 0,
    }
}

fn kinds(transcript: &Transcript) -> Vec<&'static str> {
    let kinds = transcript.entries.iter().map(|entry| match entry {
        Entry::User { .. } => "user",
        Entry::Answer(_) => "answer",
        Entry::Activity { .. } => "activity",
        Entry::Tool { .. } => "tool",
        Entry::Block(_) => "block",
        Entry::Skill { .. } => "skill",
        _ => "other",
    });
    kinds.collect()
}

fn text(piece: &str) -> ThreadEvent {
    ThreadEvent::Model(ModelEvent::Text(piece.into()))
}

fn finished(call: &ToolCall, result: ToolResult) -> ThreadEvent {
    ThreadEvent::ToolFinished {
        call_id: call.id.clone(),
        result,
    }
}

/// The same turn live and from history: a message that calls `calls` at once.
fn both_ways(calls: &[ToolCall]) -> (Transcript, Transcript) {
    let mut live = Transcript::default();
    live.add_user("go");
    live.apply(text("Here it is."));
    for call in calls {
        live.apply(ThreadEvent::ToolStarted(call.clone()));
    }
    for call in calls {
        live.apply(finished(call, result(call)));
    }
    let mut history = vec![
        message(MessageRole::User, "go", Vec::new(), None),
        message(MessageRole::Assistant, "Here it is.", calls.to_vec(), None),
    ];
    for call in calls {
        let content = result(call).content;
        history.push(message(
            MessageRole::Tool,
            &content,
            Vec::new(),
            Some(&call.id),
        ));
    }
    (live, Transcript::from_history(&history))
}

#[test]
fn a_shown_block_is_one_entry_in_place_of_its_step() {
    let (live, reopened) = both_ways(&[call("a", "show", flow())]);
    assert_eq!(kinds(&live), ["user", "answer", "block"]);
    assert_eq!(kinds(&reopened), kinds(&live));
    let shown = Block::asked(&call("a", "show", flow()));
    assert!(matches!(&live.entries[2], Entry::Block(block) if Some(block) == shown.as_ref()));
    assert_eq!(live.text(2), shown.map(|block| block.text()));
}

#[test]
fn steps_around_a_block_keep_their_own_activity_lines() {
    let calls = [
        call("a", "read_file", json!({ "path": "x" })),
        call("b", "show", flow()),
        call("c", "search_files", json!({ "pattern": "y" })),
    ];
    let (live, reopened) = both_ways(&calls);
    let expected = [
        "user", "answer", "activity", "tool", "block", "activity", "tool",
    ];
    assert_eq!(kinds(&live), expected);
    assert_eq!(kinds(&reopened), expected);
    assert_eq!(live.run(2).count(), 1);
    assert_eq!(live.run(5).count(), 1);
}

#[test]
fn a_block_that_was_not_shown_stays_a_step() {
    let out_of_caps = json!({ "flow": { "layers": [{ "title": "only one" }] } });
    let (live, reopened) = both_ways(&[call("a", "show", out_of_caps)]);
    assert_eq!(kinds(&live), ["user", "answer", "activity", "tool"]);
    assert_eq!(kinds(&reopened), kinds(&live));
}

#[test]
fn hidden_and_open_entries_stay_so_as_the_entries_move() {
    let show = call("a", "show", flow());
    let mut transcript = Transcript::default();
    transcript.add_user("go");
    transcript.apply(ThreadEvent::ToolStarted(show.clone()));
    transcript.apply(ThreadEvent::ToolStarted(call("b", "read_file", json!({}))));
    transcript.apply(text("after"));
    transcript.hide(4);
    transcript.toggle(1);
    transcript.apply(finished(&show, result(&show)));
    // user, block, activity, tool, answer: the answer is still the hidden one, and the
    // run that lost its first step keeps the open line it had.
    assert_eq!(
        kinds(&transcript),
        ["user", "block", "activity", "tool", "answer"]
    );
    assert!(transcript.is_hidden(4));
    assert!(!transcript.is_hidden(3));
}

/// Only what changed is drawn again: a streamed piece, a finished step or a new entry
/// leaves the stamps of the entries before it, so their layout is kept.
#[test]
fn only_the_entries_that_changed_get_a_new_stamp() {
    let read = call("a", "read_file", json!({ "path": "x" }));
    let mut transcript = Transcript::default();
    transcript.add_user("go");
    transcript.apply(text("Looking"));
    transcript.apply(ThreadEvent::ToolStarted(read.clone()));
    let stamps = |t: &Transcript| (0..t.entries.len()).map(|i| t.stamp(i)).collect::<Vec<_>>();
    let before = stamps(&transcript);
    transcript.apply(finished(&read, ToolResult::text("out".into())));
    let after = stamps(&transcript);
    // The step and the activity line over it are drawn again, not what came before.
    assert_eq!(after[..2], before[..2]);
    assert_ne!(after[3], before[3]);
    transcript.apply(text("Done"));
    assert_eq!(stamps(&transcript)[..4], after[..]);
    let streamed = stamps(&transcript);
    transcript.apply(text(" here"));
    assert_eq!(stamps(&transcript)[..4], streamed[..4]);
    assert!(matches!(transcript.entries.last(), Some(Entry::Answer(text)) if text == "Done here"));
}

fn change(id: i64, call_id: &str, action: SkillSaveAction, undone: bool) -> SkillChange {
    SkillChange {
        id,
        skill: "notes/outline".into(),
        call_id: call_id.into(),
        action,
        reason: "adds the outline step".into(),
        changed_at_ms: 0,
        undone,
        can_undo: !undone,
    }
}

fn skill_rows(transcript: &Transcript) -> Vec<String> {
    transcript
        .entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Skill { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_old_save_marker_in_a_tool_result_makes_no_row() {
    let mut transcript = Transcript::default();
    let save = SkillSave {
        name: "notes/outline".into(),
        action: SkillSaveAction::Created,
    };
    let run = call("a", "run_command", json!({ "command": "x" }));
    transcript.apply(ThreadEvent::ToolStarted(run.clone()));
    transcript.apply(finished(&run, ToolResult::text(save.stored())));
    assert!(skill_rows(&transcript).is_empty());
    assert!(
        !transcript
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Notice { .. }))
    );
}

#[test]
fn a_skill_line_names_the_change_and_keeps_untrusted_text_plain() {
    let mut shown = change(1, "c", SkillSaveAction::Created, true);
    assert_eq!(
        skill_line(&shown),
        "Skill notes/outline created: adds the outline step (undone)"
    );
    shown.skill = "a\x1b[2J\u{202e}b".into();
    shown.reason = "first\nsecond\u{2067}".into();
    shown.undone = false;
    shown.can_undo = true;
    shown.action = SkillSaveAction::Updated;
    assert_eq!(
        skill_line(&shown),
        "Skill a?[2J?b improved: first second (undo in the app)"
    );
    shown.action = SkillSaveAction::Deleted;
    shown.can_undo = false;
    assert_eq!(
        skill_line(&shown),
        "Skill a?[2J?b deleted: first second (previous text not kept)"
    );
}

/// The terminal used to put a change seen again in place wholesale, so a stale list
/// brought Undo back; an undo is final now, as in the other clients.
#[test]
fn a_change_seen_again_replaces_its_line_and_an_undo_stays() {
    let mut transcript = Transcript::default();
    let undone = change(1, "c", SkillSaveAction::Updated, true);
    transcript.apply(ThreadEvent::SkillChanged(change(
        1,
        "c",
        SkillSaveAction::Updated,
        false,
    )));
    transcript.apply(ThreadEvent::SkillChanged(undone));
    transcript.add_skill_changes(&[change(1, "c", SkillSaveAction::Updated, false)]);
    assert_eq!(
        skill_rows(&transcript),
        ["Skill notes/outline improved: adds the outline step (undone)"]
    );
}

#[test]
fn a_history_change_sits_under_its_step() {
    let step = call("c", "edit_skill", json!({ "name": "notes/outline" }));
    let history = [
        message(MessageRole::User, "go", Vec::new(), None),
        message(MessageRole::Assistant, "", vec![step], None),
        message(MessageRole::Tool, "saved", Vec::new(), Some("c")),
        message(MessageRole::Assistant, "done", Vec::new(), None),
    ];
    let mut transcript = Transcript::from_history(&history);
    transcript.add_skill_changes(&[change(1, "c", SkillSaveAction::Created, false)]);
    assert_eq!(
        kinds(&transcript),
        ["user", "activity", "tool", "skill", "answer"]
    );
}
