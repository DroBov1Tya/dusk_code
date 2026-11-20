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
    client::{Busy, ThreadRequest, WorkRequest},
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
        let attached = Attached::new("test".into(), Scope::full());
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

    // The open chat with a turn waiting counts as running; neither client is the owner.
    server.refresh_busy();
    assert_eq!(
        *server.busy().borrow(),
        Busy {
            clients: 2,
            running: 1
        }
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

/// A core running with the chats `a` (its folder a path that is not there) and `c` (none);
/// client 1 is bound to both, client 2 is the web and client 3 sees all. Returns the way
/// in and client 1's events.
async fn core_with_clients(
    name: &str,
) -> (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedReceiver<Event>,
) {
    use crate::client::Area;
    let root = temp_dir(name);
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_thread("a", "test").await.unwrap();
    state.create_thread("c", "test").await.unwrap();
    state
        .set_thread_project("a", "/nowhere/secret/project")
        .await
        .unwrap();
    let subscribers = Subscribers::default();
    let (queue, events) = mpsc::unbounded_channel();
    let (calls, inbox) = mpsc::unbounded_channel();
    let bound = Scope::threads(
        ["a".to_owned(), "c".to_owned()],
        &[Area::ThreadRead, Area::ThreadWrite, Area::Project],
    );
    for (client, scope) in [(1, bound), (2, Scope::web()), (3, Scope::full())] {
        let queue = match client {
            1 => queue.clone(),
            _ => mpsc::unbounded_channel().0,
        };
        let subscriber = Subscriber {
            client,
            scope: scope.clone(),
            queue,
        };
        subscribers.lock().push(subscriber);
        let attach = Message::Attach {
            client,
            source: "test".into(),
            scope,
            transport: "in-process",
            owner: false,
        };
        calls.send(attach).unwrap();
    }
    let (server, _) = Server::new(runtime, subscribers);
    tokio::spawn(server.run(inbox));
    (calls, events)
}

async fn ask(
    calls: &mpsc::UnboundedSender<Message>,
    client: ClientId,
    request: Request,
) -> Response {
    let (reply, answer) = oneshot::channel();
    let call = Call {
        client,
        request,
        reply,
    };
    calls.send(Message::Call(call)).unwrap();
    answer.await.unwrap()
}

fn resume(thread_id: &str) -> Request {
    Request::Thread(ThreadRequest::Resume {
        thread_id: thread_id.into(),
        cwd: "/nowhere/secret/project".into(),
    })
}

/// What a handler fails with can hold paths: a client that is not the user's sees a
/// reference, and the log has the rest.
#[tokio::test]
async fn a_bound_client_sees_a_ref_not_a_path() {
    let (calls, _events) = core_with_clients("serve_ref").await;
    let Response::Error(text) = ask(&calls, 1, resume("a")).await else {
        panic!("the diff of a folder that is not there fails");
    };
    assert!(is_a_ref_text(&text), "{text}");
    assert!(!text.contains("nowhere"));
    // The user's own clients read the whole text.
    for client in [2, 3] {
        let Response::Error(text) = ask(&calls, client, resume("a")).await else {
            panic!("it fails for everyone");
        };
        assert!(!text.starts_with("the request failed"), "{text}");
    }
}

/// `^the request failed \(ref [0-9a-f]{8}\)$`
fn is_a_ref_text(text: &str) -> bool {
    text.strip_prefix("the request failed (ref ")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|id| id.len() == 8 && id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')))
}

#[tokio::test]
async fn the_web_scope_keeps_error_text() {
    let (calls, _events) = core_with_clients("serve_web_text").await;
    let Response::Error(text) = ask(&calls, 2, resume("a")).await else {
        panic!("it fails");
    };
    assert!(
        text.starts_with("working directory is not accessible"),
        "{text}"
    );
}

#[tokio::test]
async fn a_shown_error_passes() {
    let (calls, _events) = core_with_clients("serve_shown").await;
    // A chat with no folder stored: the sentence meant for the client passes as it is.
    let response = ask(&calls, 1, resume("c")).await;
    assert_eq!(
        response,
        Response::Error("this chat cannot be opened from this client".into())
    );
    let error = anyhow::Error::new(Shown::new("the chat is busy")).context("deeper");
    let text = shown::shown_text(&error, false, "test", crate::client::Area::ThreadWrite);
    assert_eq!(text, "the chat is busy");
}

/// The 21st refused request within a minute detaches the client: its queue closes.
#[tokio::test]
async fn the_twenty_first_refusal_detaches() {
    let (calls, mut events) = core_with_clients("serve_refusals").await;
    // A scope bound to chats may not ask for the system report.
    for count in 1..=REFUSAL_LIMIT + 1 {
        let response = ask(&calls, 1, Request::SystemReport).await;
        assert!(matches!(response, Response::Error(_)), "{count}");
        if count <= REFUSAL_LIMIT {
            assert!(
                events
                    .try_recv()
                    .is_err_and(|e| e == mpsc::error::TryRecvError::Empty),
                "{count}"
            );
        }
    }
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap(),
        None
    );
    // The web, which may ask it, is not affected.
    assert!(!matches!(
        ask(&calls, 2, Request::SystemReport).await,
        Response::Error(_)
    ));
}

/// The transport asks whether a client was detached for its refusals, to tell it so.
#[tokio::test]
async fn a_client_detached_for_refusals_is_known_until_its_handle_goes() {
    let root = temp_dir("serve_refused_out");
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let (mut server, _) = Server::new(runtime, Subscribers::default());
    for client in [1, 2] {
        let attached = Attached::new("test".into(), Scope::full());
        server.clients.insert(client, attached);
    }
    for _ in 0..REFUSAL_LIMIT {
        server.count_refusal(1).await;
    }
    assert!(!server.refused_too_often(1));
    server.count_refusal(1).await;
    assert!(server.refused_too_often(1));
    assert!(!server.clients.contains_key(&1));
    assert!(!server.refused_too_often(2));
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

/// The suggestion a joining client is told follows the chat's own events: the newest
/// one, gone once the user says something or a new turn starts; a subagent's never.
#[test]
fn the_kept_suggestion_follows_the_chats_events() {
    use super::thread::suggestion_after;
    let suggested = |text: &str| ThreadEvent::NextSuggested { text: text.into() };
    let mut kept = None;
    let mut hear = |event: ThreadEvent| {
        if let Some(next) = suggestion_after(&event) {
            kept = next;
        }
        kept.clone()
    };
    assert_eq!(
        hear(suggested("run the tests")).as_deref(),
        Some("run the tests")
    );
    assert_eq!(hear(ThreadEvent::Idle).as_deref(), Some("run the tests"));
    let wrapped = ThreadEvent::Subagent {
        thread_id: "child".into(),
        event: Box::new(suggested("from a subagent")),
    };
    assert_eq!(hear(wrapped).as_deref(), Some("run the tests"));
    let sent = ThreadEvent::Queued {
        message_id: "m".into(),
    };
    assert_eq!(hear(sent), None);
    hear(suggested("commit it"));
    assert_eq!(hear(ThreadEvent::Undone { text: "x".into() }), None);
}

/// The end of a turn marks a chat no client showed unread, and every client listing it
/// hears so; the user's own message, progress, a subagent's turn and a turn the user
/// stopped do not, and a limit that stopped one does. A client that
/// shows the chat marks it read, and the others hear that too.
#[tokio::test]
async fn a_chat_turns_unread_for_the_user_and_read_once_seen() {
    let root = temp_dir("serve_unread");
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap();
    let state = runtime.state();
    state.create_thread("chat", "test").await.unwrap();
    state.add_user_message("chat", "go", "go").await.unwrap();
    let turn = state.start_turn("chat").await.unwrap().unwrap();
    let subscribers = Subscribers::default();
    let (queue, mut heard) = mpsc::unbounded_channel();
    subscribers.lock().push(Subscriber {
        client: 1,
        scope: Scope::full(),
        queue,
    });
    let (other, mut elsewhere) = mpsc::unbounded_channel();
    subscribers.lock().push(Subscriber {
        client: 2,
        scope: Scope::threads(["another".to_owned()], &[crate::client::Area::ThreadRead]),
        queue: other,
    });
    let (mut server, _) = Server::new(runtime, Arc::clone(&subscribers));
    server
        .clients
        .insert(1, Attached::new("test".into(), Scope::full()));
    server.caller = 1;
    let unread = |server: &Server| {
        let runtime = Arc::clone(&server.runtime);
        async move {
            let listed = runtime.list_threads().await.unwrap();
            listed
                .iter()
                .any(|thread| thread.id == "chat" && thread.unread)
        }
    };

    let not_for_the_user = [
        ThreadEvent::Queued {
            message_id: "m".into(),
        },
        ThreadEvent::TurnStarted(turn.clone()),
        ThreadEvent::Subagent {
            thread_id: "agent".into(),
            event: Box::new(ThreadEvent::TurnFinished(turn.clone())),
        },
    ];
    for event in &not_for_the_user {
        super::unread::heard(&server.runtime, &subscribers, "chat", event).await;
    }
    assert!(!unread(&server).await);
    assert!(heard.try_recv().is_err());

    // A turn the user stopped: they did that themselves.
    let mut stopped = turn.clone();
    stopped.status = dusk_protocol::thread::TurnStatus::Interrupted;
    let stopped = ThreadEvent::TurnFinished(stopped);
    super::unread::heard(&server.runtime, &subscribers, "chat", &stopped).await;
    assert!(!unread(&server).await, "a turn the user stopped");
    assert!(heard.try_recv().is_err());

    let mut finished = turn.clone();
    finished.status = dusk_protocol::thread::TurnStatus::Completed;
    let finished = ThreadEvent::TurnFinished(finished);
    super::unread::heard(&server.runtime, &subscribers, "chat", &finished).await;
    assert!(unread(&server).await);
    assert_eq!(
        next_thread_event(&mut heard).await,
        ThreadEvent::Unread(true)
    );
    super::unread::heard(&server.runtime, &subscribers, "chat", &finished).await;
    assert!(heard.try_recv().is_err(), "told once");

    let seen = ThreadRequest::Seen {
        thread_id: "chat".into(),
    };
    assert_eq!(server.thread(seen).await.unwrap(), Response::Done);
    assert!(!unread(&server).await);
    assert_eq!(
        next_thread_event(&mut heard).await,
        ThreadEvent::Unread(false)
    );
    assert!(
        elsewhere.try_recv().is_err(),
        "a client bound elsewhere hears nothing"
    );

    // A limit that stopped the turn is for the user, though the turn ends interrupted.
    let limit = ThreadEvent::OutputLimitReached;
    super::unread::heard(&server.runtime, &subscribers, "chat", &limit).await;
    assert!(unread(&server).await, "stopped by a limit");
    assert_eq!(
        next_thread_event(&mut heard).await,
        ThreadEvent::Unread(true)
    );
}
