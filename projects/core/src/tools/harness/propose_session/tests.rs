//! A proposal is checked, capped, and never starts anything by itself.

use serde_json::json;

use super::*;
use crate::{
    testing::{temp_dir, tool_context},
    tools::{Progress, ToolScope},
};

struct Rig {
    state: Arc<State>,
    scope: ToolScope,
    modes: PathBuf,
}

async fn rig(name: &str) -> Rig {
    let dir = temp_dir(name);
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::create_dir_all(dir.join("modes")).unwrap();
    let state = Arc::new(State::open(&dir.join("state.db")).await.unwrap());
    state.create_thread("chat", "test").await.unwrap();
    state.create_thread("other", "test").await.unwrap();
    let scope = ToolScope::new(&dir.join("project"), &dir.join("project")).unwrap();
    Rig {
        state,
        scope,
        modes: dir.join("modes"),
    }
}

impl Rig {
    /// A call as the chat makes it; its result is stored as the runtime stores it.
    async fn propose(&self, call_id: &str, arguments: Value) -> Result<String> {
        self.propose_in("chat", call_id, arguments).await
    }

    /// The same in the chat `thread_id`, whose message gets an id of its own while the
    /// model's call id may repeat.
    async fn propose_in(&self, thread_id: &str, call_id: &str, arguments: Value) -> Result<String> {
        let (events, _heard) = tokio::sync::mpsc::unbounded_channel();
        let context = ToolContext {
            state: Some(Arc::clone(&self.state)),
            thread_id: thread_id.into(),
            progress: Some(Progress {
                call_id: call_id.into(),
                events,
            }),
            ..tool_context(&self.scope)
        };
        let result = ProposeSession::new(self.modes.clone())
            .call(arguments, context)
            .await?;
        sqlx::query(
            "INSERT INTO messages (message_id, thread_id, sequence, role, content,
                tool_call_id, content_tokens, tool_calls_tokens, reasoning_tokens, created_at)
             VALUES (?1, ?2,
                (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = ?2),
                'tool', ?3, ?4, 0, 0, 0, 1)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(thread_id)
        .bind(&result)
        .bind(call_id)
        .execute(&self.state.pool)
        .await
        .unwrap();
        Ok(result)
    }

    /// The id of the proposal a call made.
    async fn proposed(&self, thread_id: &str, call_id: &str, arguments: Value) -> String {
        let shown = self
            .propose_in(thread_id, call_id, arguments)
            .await
            .unwrap();
        Proposal::read(&shown).unwrap().id
    }
}

fn brief(title: &str) -> Value {
    json!({ "title": title, "brief": "Make the gate run pytest, not uv run pytest." })
}

#[tokio::test]
async fn a_proposal_is_checked_and_stored_as_its_card() {
    let rig = rig("propose-check").await;
    for wrong in [
        json!({ "title": " ", "brief": "x" }),
        json!({ "title": "t", "brief": "  " }),
        json!({ "title": "x".repeat(MAX_PROPOSAL_TITLE + 1), "brief": "x" }),
        json!({ "title": "t", "brief": "x".repeat(MAX_PROPOSAL_BRIEF + 1) }),
        json!({ "title": "t", "brief": "x", "project": "nowhere" }),
        json!({ "title": "t", "brief": "x", "mode": "ROOT" }),
        json!({ "title": "t", "brief": "x", "branch": "a b; rm -rf" }),
        json!({ "title": "t", "brief": "x", "start": true }),
    ] {
        assert!(rig.propose("w", wrong.clone()).await.is_err(), "{wrong}");
    }
    let shown = rig
        .propose(
            "c1",
            json!({ "title": "Fix\u{202e} gate\nnow", "brief": " b ", "mode": "PLAN", "branch": "fix/gate" }),
        )
        .await
        .unwrap();
    let card = Proposal::read(&shown).unwrap();
    assert_eq!(card.title, "Fix gate");
    assert_eq!(card.brief, "b");
    assert_eq!(card.mode.as_deref(), Some("PLAN"));
    let (found, state) = find(&rig.state, "chat", &card.id).await.unwrap();
    assert_eq!((found, state), (card, ProposalState::Pending));
}

/// Waiting proposals are capped; one answered frees its place, and an answer is kept
/// once, for the next turn, from the chat itself.
#[tokio::test]
async fn pending_proposals_are_capped_and_an_answer_counts_once() {
    let rig = rig("propose-cap").await;
    let mut ids = Vec::new();
    for at in 0..MAX_PENDING_PROPOSALS {
        ids.push(rig.proposed("chat", &format!("c{at}"), brief("t")).await);
    }
    let refused = rig.propose("over", brief("t")).await.unwrap_err();
    assert!(
        format!("{refused:#}").contains("wait for their answer"),
        "{refused:#}"
    );

    let state = ProposalState::Dismissed;
    let first = &ids[0];
    assert!(
        settle(&rig.state, "chat", first, "t", &state)
            .await
            .unwrap()
    );
    let again = ProposalState::Started {
        thread_id: "chat".into(),
    };
    assert!(
        !settle(&rig.state, "chat", first, "t", &again)
            .await
            .unwrap()
    );
    assert_eq!(find(&rig.state, "chat", first).await.unwrap().1, state);
    rig.propose("next", brief("t")).await.unwrap();

    // Waiting for the next turn: from the chat, so never a command nor an answer.
    let pending = rig.state.pending_messages("chat").await.unwrap();
    let [note] = pending.as_slice() else {
        panic!("{pending:?}")
    };
    assert_eq!(note.from_thread_id.as_deref(), Some("chat"));
    assert_eq!(read_outcome(&note.content).unwrap().1, state);
}

/// Only a chat with a user may propose; a subagent is not offered the tool.
#[tokio::test]
async fn nothing_to_propose_to_without_a_user() {
    let rig = rig("propose-nobody").await;
    let context = ToolContext {
        state: Some(Arc::clone(&rig.state)),
        thread_id: "chat".into(),
        ..tool_context(&rig.scope)
    };
    let tool = ProposeSession::new(rig.modes.clone());
    assert!(tool.call(brief("t"), context).await.is_err());
    assert!(tool.root_only());
}

/// Models that number their calls `call_0` in every turn, or leave the id empty, still
/// get proposals told apart: across chats, and within one chat across turns.
#[tokio::test]
async fn repeated_call_ids_name_different_proposals() {
    let rig = rig("propose-repeated").await;
    let first = rig.proposed("chat", "call_0", brief("first")).await;
    let elsewhere = rig.proposed("other", "call_0", brief("elsewhere")).await;
    let later = rig.proposed("chat", "call_0", brief("later")).await;
    assert!(first != later && first != elsewhere && later != elsewhere);

    let dismissed = ProposalState::Dismissed;
    assert!(
        settle(&rig.state, "chat", &first, "first", &dismissed)
            .await
            .unwrap()
    );
    // The other chat's and the later card still wait, each with its own brief.
    let (card, state) = find(&rig.state, "other", &elsewhere).await.unwrap();
    assert_eq!(
        (card.title.as_str(), state),
        ("elsewhere", ProposalState::Pending)
    );
    let (card, state) = find(&rig.state, "chat", &later).await.unwrap();
    assert_eq!(
        (card.title.as_str(), state),
        ("later", ProposalState::Pending)
    );
    assert_eq!(find(&rig.state, "chat", &first).await.unwrap().1, dismissed);
    // A card is found only in its own chat.
    assert!(find(&rig.state, "other", &first).await.is_err());

    let started = ProposalState::Started {
        thread_id: "new".into(),
    };
    assert!(
        settle(&rig.state, "other", &elsewhere, "elsewhere", &started)
            .await
            .unwrap()
    );
    assert!(
        settle(&rig.state, "chat", &later, "later", &started)
            .await
            .unwrap()
    );
    assert_eq!(pending(&rig.state, "chat").await.unwrap(), 0);
}
