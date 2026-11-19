//! A client that joins a thread another client keeps open hears what it missed; a
//! code block's run stopped by its client is killed with what it started.

use super::*;
use crate::{
    client::Scope,
    config::ApprovalPolicy,
    init::paths::Paths,
    providers::ToolCall,
    state::TurnAnswer,
    testing::temp_dir,
    tools::{Approvals, Progress},
};
use dusk_protocol::{
    client::{ThreadRequest, WorkRequest},
    thread::{ThreadEvent, TurnWait},
    tools::{Decision, Request as ToolRequest},
};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

async fn next_thread_event(queue: &mut mpsc::UnboundedReceiver<Event>) -> ThreadEvent {
    loop {
        if let Some(Event::Thread { event, .. }) = queue.recv().await {
            return *event;
        }
    }
}

/// The turn a restart cut off waits for the user; a client that comes later is asked too.
#[tokio::test]
async fn a_client_that_joins_a_waiting_turn_gets_the_wait_and_the_question() {
    let root = temp_dir("serve_join_waiting");
    let cwd = root.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_thread("chat", "test").await.unwrap();
    state.add_user_message("chat", "go", "go").await.unwrap();
    let turn = state.start_turn("chat").await.unwrap().unwrap();
    let call = ToolCall {
        id: "run".into(),
        name: "run_command".into(),
        arguments: serde_json::json!({ "command": "make" }),
    };
    let answer = TurnAnswer {
        content: String::new(),
        reasoning: None,
    };
    state
        .record_tool_calls(&turn.turn_id, answer, vec![call])
        .await
        .unwrap();

    let subscribers = Subscribers::default();
    let (first, mut heard_first) = mpsc::unbounded_channel();
    let subscriber = |client, queue| Subscriber {
        client,
        scope: Scope::full(),
        queue,
    };
    subscribers.lock().push(subscriber(1, first));
    let (mut server, _) = Server::new(runtime, Arc::clone(&subscribers));
    for client in [1, 2] {
        let attached = Attached {
            source: "test".into(),
            scope: Scope::full(),
        };
        server.clients.insert(client, attached);
    }
    let resume = || ThreadRequest::Resume {
        thread_id: "chat".into(),
        cwd: cwd.clone(),
    };
    server.caller = 1;
    server.thread(resume()).await.unwrap();
    let asked = loop {
        if let ThreadEvent::Question(question) = next_thread_event(&mut heard_first).await {
            break question;
        }
    };

    let (second, mut heard_second) = mpsc::unbounded_channel();
    subscribers.lock().push(subscriber(2, second));
    server.caller = 2;
    server.thread(resume()).await.unwrap();

    let ThreadEvent::TurnWaiting(waiting) = next_thread_event(&mut heard_second).await else {
        panic!("the wait comes first");
    };
    assert_eq!(waiting.turn_id, turn.turn_id);
    assert!(matches!(waiting.waiting, Some(TurnWait::Decision { .. })));
    assert_eq!(
        next_thread_event(&mut heard_second).await,
        ThreadEvent::Question(asked)
    );
}

/// Stop kills the command and its children at once, and the run answers that it stopped.
#[tokio::test]
async fn a_stopped_run_is_killed_with_what_it_started() {
    let root = temp_dir("serve_stop_run");
    let cwd = root.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let cwd = cwd.canonicalize().unwrap();
    let dir = cwd.to_str().unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    std::fs::create_dir_all(paths.thread_workspace("chat").unwrap()).unwrap();
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_project("p", "p", "", dir).await.unwrap();
    state.create_thread("chat", "test").await.unwrap();
    state.set_thread_project("chat", dir).await.unwrap();
    let (server, _) = Server::new(runtime, Subscribers::default());

    let command = "(sleep 3; touch child_survived) & sleep 3 && touch survived; wait";
    let (reply, mut answer) = oneshot::channel();
    server.run_command(1, "chat".into(), command.into(), 7, reply);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let early = answer.try_recv();
    assert!(early.is_err(), "the command is still running: {early:?}");
    assert!(
        !server.stop_run(2, "chat".into(), 7),
        "only its client stops it"
    );
    assert!(server.stop_run(1, "chat".into(), 7));

    let response = tokio::time::timeout(Duration::from_secs(1), answer)
        .await
        .expect("a stopped run answers at once")
        .unwrap();
    assert!(
        matches!(&response, Response::Error(error) if error.contains("stopped by the user")),
        "{response:?}"
    );
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert!(!cwd.join("survived").exists());
    assert!(!cwd.join("child_survived").exists());
}

/// Starts a core with chats `a` and `b`, and the agent thread `a1` below `a`.
async fn rig_with_chats(name: &str) -> (Server, Arc<Approvals>) {
    let root = temp_dir(name);
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_thread("a", "test").await.unwrap();
    state.create_thread("b", "test").await.unwrap();
    state
        .create_agent_thread("a1", "a", "reviewer", "check")
        .await
        .unwrap();
    let approvals = runtime.approvals();
    let (server, _) = Server::new(runtime, Subscribers::default());
    (server, approvals)
}

/// Asks for a yes in `thread`; gives the request number and the task that waits for it.
async fn ask_in(
    approvals: &Arc<Approvals>,
    thread: &str,
) -> (u64, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (events, mut seen) = mpsc::unbounded_channel();
    let progress = Progress {
        call_id: "c".into(),
        events,
    };
    let (approvals, thread) = (Arc::clone(approvals), thread.to_owned());
    let waiting = tokio::spawn(async move {
        let request = ToolRequest::Command {
            command: "make".into(),
        };
        approvals
            .ask(&thread, request, Some(progress), ApprovalPolicy::Ask)
            .await
    });
    let Some(ThreadEvent::Approval(asked)) = seen.recv().await else {
        panic!("missing request");
    };
    (asked.id, waiting)
}

async fn approve(server: &mut Server, thread: &str, id: u64) -> Response {
    let request = ThreadRequest::Approve {
        thread_id: thread.into(),
        id,
        decision: Decision::Once,
    };
    server.thread(request).await.unwrap()
}

#[tokio::test]
async fn approve_with_another_thread_answers_false() {
    let (mut server, approvals) = rig_with_chats("serve_approve_other").await;
    let (id, waiting) = ask_in(&approvals, "a").await;
    assert!(matches!(
        approve(&mut server, "b", id).await,
        Response::Waiting(false)
    ));
    assert!(!waiting.is_finished(), "the turn still waits");
    assert!(matches!(
        approve(&mut server, "a", id).await,
        Response::Waiting(true)
    ));
    waiting.await.unwrap().unwrap();
}

#[tokio::test]
async fn approve_for_a_subagent_of_the_named_chat_answers() {
    let (mut server, approvals) = rig_with_chats("serve_approve_agent").await;
    let (id, waiting) = ask_in(&approvals, "a1").await;
    assert!(matches!(
        approve(&mut server, "a", id).await,
        Response::Waiting(true)
    ));
    waiting.await.unwrap().unwrap();
}

#[tokio::test]
async fn approve_with_an_unknown_id_answers_false() {
    let (mut server, _) = rig_with_chats("serve_approve_unknown").await;
    assert!(matches!(
        approve(&mut server, "a", 99).await,
        Response::Waiting(false)
    ));
}

/// A core with chats `a` and `b` and the agent thread `a1` below `a`, each holding a card
/// in a message of its own; the cards' ids are in that order.
async fn rig_with_cards(name: &str) -> (Server, [String; 3]) {
    use crate::state::artifacts::{ArtifactStore, Card};
    let root = temp_dir(name);
    let paths = Paths::new(root.join("config"), root.join("work"));
    let store = ArtifactStore::new(paths.artifacts_dir());
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_thread("a", "test").await.unwrap();
    state.create_thread("b", "test").await.unwrap();
    state
        .create_agent_thread("a1", "a", "reviewer", "check")
        .await
        .unwrap();
    let mut ids = Vec::new();
    for thread in ["a", "a1", "b"] {
        let card = store
            .save(Card::default(), thread.as_bytes(), b"small")
            .unwrap();
        ids.push(card.id.clone());
        let pending = state.add_pending(thread, "go").await.unwrap();
        state.admit_pending(thread, &pending).await.unwrap();
        let turn = state.start_turn(thread).await.unwrap().unwrap().turn_id;
        let call = ToolCall {
            id: "c".into(),
            name: "display_image".into(),
            arguments: serde_json::json!({}),
        };
        let answer = TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        state
            .record_tool_calls(&turn, answer, vec![call])
            .await
            .unwrap();
        let result =
            state.record_tool_result_with_artifacts(&turn, "c", "shown".into(), vec![card]);
        result.await.unwrap();
    }
    let (server, _) = Server::new(runtime, Subscribers::default());
    (server, ids.try_into().unwrap())
}

async fn card_of(server: &Server, thread: &str, card: &str, part: &str) -> Response {
    let request = WorkRequest::Card {
        thread_id: thread.into(),
        card_id: card.into(),
        part: part.into(),
    };
    server.work(request).await.unwrap()
}

#[tokio::test]
async fn a_card_of_the_thread_is_served() {
    let (server, [own, agent, _]) = rig_with_cards("serve_card_own").await;
    let Response::Picture(bytes) = card_of(&server, "a", &own, "image").await else {
        panic!("not served");
    };
    assert_eq!(bytes, b"a");
    // A card of an agent thread below the thread, and a part other than the original.
    let Response::Picture(bytes) = card_of(&server, "a", &agent, "thumb").await else {
        panic!("not served");
    };
    assert_eq!(bytes, b"small");
}

#[tokio::test]
async fn a_card_of_another_thread_is_refused() {
    let (server, [own, agent, other]) = rig_with_cards("serve_card_other").await;
    for (thread, card) in [("a", &other), ("b", &own), ("b", &agent), ("a1", &own)] {
        let answer = card_of(&server, thread, card, "image").await;
        assert!(matches!(answer, Response::Error(ref e) if e == "no such card"));
    }
}

#[tokio::test]
async fn a_card_id_with_a_path_is_refused() {
    let (server, [own, ..]) = rig_with_cards("serve_card_path").await;
    for card in [
        format!("../{own}"),
        format!("{own}/.."),
        format!("{own}\\x"),
        "..".to_owned(),
        String::new(),
    ] {
        let answer = card_of(&server, "a", &card, "image").await;
        assert!(matches!(answer, Response::Error(_)), "{card}");
    }
    let answer = card_of(&server, "a", &own, "../image").await;
    assert!(matches!(answer, Response::Error(_)));
}
