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
        let (events, _heard) = tokio::sync::mpsc::unbounded_channel();
        let context = ToolContext {
            state: Some(Arc::clone(&self.state)),
            thread_id: "chat".into(),
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
             VALUES (?1, 'chat',
                (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE thread_id = 'chat'),
                'tool', ?2, ?1, 0, 0, 0, 1)",
        )
        .bind(call_id)
        .bind(&result)
        .execute(&self.state.pool)
        .await
        .unwrap();
        Ok(result)
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
    let (found, state) = find(&rig.state, "chat", "c1").await.unwrap();
    assert_eq!((found, state), (card, ProposalState::Pending));
}

/// Waiting proposals are capped; one answered frees its place, and an answer is kept
/// once, for the next turn, from the chat itself.
#[tokio::test]
async fn pending_proposals_are_capped_and_an_answer_counts_once() {
    let rig = rig("propose-cap").await;
    for at in 0..MAX_PENDING_PROPOSALS {
        rig.propose(&format!("c{at}"), brief("t")).await.unwrap();
    }
    let refused = rig.propose("over", brief("t")).await.unwrap_err();
    assert!(
        format!("{refused:#}").contains("wait for their answer"),
        "{refused:#}"
    );

    let state = ProposalState::Dismissed;
    assert!(settle(&rig.state, "chat", "c0", "t", &state).await.unwrap());
    let again = ProposalState::Started {
        thread_id: "chat".into(),
    };
    assert!(!settle(&rig.state, "chat", "c0", "t", &again).await.unwrap());
    assert_eq!(find(&rig.state, "chat", "c0").await.unwrap().1, state);
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
