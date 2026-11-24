//! What the page keeps beside the protocol's chat; the chat's own rules are tested in the
//! protocol.

use super::*;
use dusk_protocol::{
    model::ModelEvent,
    thread::{MessageRole, Turn, TurnStatus},
    tools::Question,
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
        chat.questions.shown().map(|q| q.question.as_str()),
        Some("one")
    );
    assert_eq!(chat.questions.place(), (1, 2));
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
    let mut chat = ChatData::default();
    chat.apply(ThreadEvent::Question(first.clone()));
    chat.apply(ThreadEvent::Question(second.clone()));
    assert_eq!(chat.answer_questions(QuestionQueue::skip), None);
    assert_eq!(chat.questions.shown(), Some(&second));
    let sent = chat.answer_questions(QuestionQueue::skip).unwrap();
    assert_eq!(sent, [first.skipped(), second.skipped()].join("\n\n"));
    assert!(chat.questions.is_empty());
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

#[test]
fn the_goal_follows_the_core_and_its_command_leaves_the_queue() {
    use dusk_protocol::thread::{Goal, GoalEvaluation, GoalStatus, GoalVerdict};
    let mut chat = ChatData::default();
    chat.sent("/goal all tests pass".into());
    let goal = Goal {
        thread_id: "t".into(),
        text: "all tests pass".into(),
        status: GoalStatus::Active,
        started_at: 0,
        turns: 0,
        tokens: 0,
        reason: None,
        note: None,
    };
    chat.apply(ThreadEvent::Goal {
        goal: Some(goal),
        note: String::new(),
        started: Some("all tests pass".into()),
    });
    assert!(chat.pending.is_empty());
    chat.apply(ThreadEvent::GoalEvaluated(GoalEvaluation {
        verdict: GoalVerdict::NotMet,
        reason: "run the tests".into(),
        turn: 1,
        tokens: 900,
        elapsed_ms: 1000,
    }));
    let goal = chat.goal.as_ref().expect("still set");
    assert_eq!((goal.turns, goal.tokens), (1, 900));
    assert_eq!(goal.reason.as_deref(), Some("run the tests"));
    chat.sent("/goal clear".into());
    chat.apply(ThreadEvent::Goal {
        goal: None,
        note: "Goal cleared: all tests pass".into(),
        started: None,
    });
    assert!(chat.goal.is_none() && chat.pending.is_empty());
}

#[test]
fn a_goal_that_ended_while_offline_goes_on_reconnect() {
    use dusk_protocol::thread::{Goal, GoalStatus};
    let goal = Goal {
        thread_id: "t".into(),
        text: "all tests pass".into(),
        status: GoalStatus::Active,
        started_at: 0,
        turns: 2,
        tokens: 0,
        reason: None,
        note: None,
    };
    let told = |goal: Option<Goal>| ThreadEvent::Goal {
        goal,
        note: String::new(),
        started: None,
    };
    let mut chat = ChatData::default();
    chat.apply(told(Some(goal.clone())));
    // The goal was met while the stream was down: the reopening tells none.
    chat.reopening();
    chat.reloaded(&[]);
    assert!(chat.goal.is_none());
    // A goal still set comes with the reopening, before the history is read again.
    chat.reopening();
    chat.apply(told(Some(goal.clone())));
    chat.reloaded(&[]);
    assert_eq!(chat.goal, Some(goal));
}
