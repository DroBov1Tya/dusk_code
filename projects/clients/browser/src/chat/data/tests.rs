//! What the page keeps beside the protocol's chat; the chat's own rules are tested in the
//! protocol.

use super::*;
use dusk_protocol::{
    model::ModelEvent,
    thread::{MessageRole, Turn, TurnStatus},
};

fn message(role: MessageRole, content: &str) -> Message {
    Message {
        message_id: "m".into(),
        thread_id: "t".into(),
        sequence: 1,
        role,
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
        created_at: 0,
    }
}

fn question(text: &str) -> Question {
    Question {
        question: text.into(),
        options: vec!["A".into()],
        multiple: false,
    }
}

#[test]
fn a_sent_message_is_queued_until_the_core_takes_it_in() {
    let mut chat = ChatData::default();
    chat.sent("hello".into());
    assert_eq!(chat.pending, ["hello"]);
    chat.apply(ThreadEvent::UserMessage(message(
        MessageRole::User,
        "hello",
    )));
    assert!(chat.pending.is_empty());
    assert!(matches!(&chat.items[0], Item::User { text, .. } if text == "hello"));
}

#[test]
fn a_second_question_waits_behind_the_first() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::Question(question("one")));
    chat.apply(ThreadEvent::Question(question("two")));
    assert_eq!(
        chat.question.as_ref().map(|q| q.question.as_str()),
        Some("one")
    );
    assert_eq!(chat.questions_waiting.len(), 1);
}

#[test]
fn a_streamed_piece_is_told_to_its_item_alone() {
    let turn = Turn {
        turn_id: "turn".into(),
        thread_id: "t".into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: 0,
        finished_at: None,
        error_message: None,
        waiting: None,
        steps: 0,
        tokens: 0,
        cost_micro_usd: 0,
    };
    let mut chat = ChatData::default();
    let started = chat.apply(ThreadEvent::TurnStarted(turn.clone()));
    assert!(!chat.told(&started));
    let text = |piece: &str| ThreadEvent::Model(ModelEvent::Text(piece.into()));
    // The first words make the answer drawn: rows move, the whole chat is told.
    let first = chat.apply(text("Hel"));
    assert!(!chat.told(&first));
    let next = chat.apply(text("lo"));
    assert!(chat.told(&next));
    assert!(matches!(&chat.items[0], Item::Assistant { text, .. } if text == "Hello"));
    chat.apply(ThreadEvent::TurnFinished(turn));
    assert_eq!(chat.moved, 1);
}

#[test]
fn skipping_moves_on_and_the_last_skip_sends_every_answer() {
    let ask = |text: &str| Question {
        question: text.into(),
        options: vec!["a".into(), "b".into()],
        multiple: false,
    };
    let (first, second) = (ask("One?"), ask("Two?"));
    let mut chat = ChatData {
        question: Some(first.clone()),
        questions_waiting: [second.clone()].into(),
        ..ChatData::default()
    };
    assert_eq!(chat.record_answer(first.skipped()), None);
    assert_eq!(chat.question, Some(second.clone()));
    let sent = chat.record_answer(second.skipped()).unwrap();
    assert_eq!(sent, [first.skipped(), second.skipped()].join("\n\n"));
    assert!(chat.question.is_none() && chat.answers.is_empty());
}

#[test]
fn the_composer_offers_the_chats_own_suggestion_until_the_user_speaks() {
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::Subagent {
        thread_id: "child".into(),
        event: Box::new(ThreadEvent::NextSuggested {
            text: "from a subagent".into(),
        }),
    });
    assert_eq!(chat.suggestion.ghost(""), None);
    chat.apply(ThreadEvent::NextSuggested {
        text: "commit it".into(),
    });
    assert_eq!(chat.suggestion.ghost(""), Some("commit it"));
    // Typing hides it and Tab over text keeps its meaning; the empty box shows it again.
    assert_eq!(chat.suggestion.ghost("c"), None);
    assert_eq!(chat.suggestion.accept("c"), None);
    assert_eq!(chat.suggestion.accept("").as_deref(), Some("commit it"));
    chat.apply(ThreadEvent::Queued {
        message_id: "m".into(),
    });
    assert_eq!(chat.suggestion.ghost(""), None);
}
