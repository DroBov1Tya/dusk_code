//! A stall candidate in real turns: a judge sure the turn is stuck ends it with what was
//! tried; a judge that is not lets it go on with its advice as a note; work that moves
//! on is never judged.

use super::*;
use dusk_protocol::thread::RepeatRung;

fn notices(events: &[ThreadEvent]) -> Vec<(RepeatRung, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect()
}

/// `steps` steps of one `echo` call each, the text from `text`.
fn calls(steps: usize, text: impl Fn(usize) -> String) -> Vec<Vec<Step>> {
    (0..steps)
        .map(|i| {
            let id = format!("call_{i}");
            vec![Step::Echo(0, id, json!({ "text": text(i) })), Step::Done]
        })
        .collect()
}

/// Two calls in turn with the same results: the cycle asks for a re-plan at its 10th
/// call and again at its 11th, which makes the turn a stall candidate.
fn cycle(steps: usize) -> Vec<Vec<Step>> {
    calls(steps, |i| {
        if i % 2 == 0 { "git status" } else { "git diff" }.to_owned()
    })
}

fn judge(answer: &'static str) -> Vec<Step> {
    vec![Step::Text(answer), Step::Done]
}

async fn run(
    name: &str,
    settings: &str,
    scripts: Vec<Vec<Step>>,
) -> (Arc<FakeProvider>, Vec<ThreadEvent>, Turn) {
    let config = temp_live_config_with(&format!("{name}_config"), settings);
    run_with(name, config, scripts).await
}

async fn run_with(
    name: &str,
    config: LiveConfig,
    scripts: Vec<Vec<Step>>,
) -> (Arc<FakeProvider>, Vec<ThreadEvent>, Turn) {
    let state = thread_state(name).await;
    let config = Arc::new(config);
    let provider = provider(scripts);
    let (runner, input, mut output) = runner(&state, &provider, &config, model(None));
    input.send("tidy the repo").await.unwrap();
    drop(input);
    runner.run().await;
    let events = drain(&mut output).await;
    let turn = events
        .iter()
        .find_map(|event| match event {
            ThreadEvent::TurnFinished(turn) => Some(turn.clone()),
            _ => None,
        })
        .expect("a finished turn");
    (provider, events, turn)
}

/// The requests sent without tools, the chat's title aside: the judge's.
fn judge_requests(provider: &FakeProvider) -> Vec<Vec<ChatMessage>> {
    provider
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|messages| {
            matches!(messages.first(), Some(ChatMessage::System(prompt))
                if prompt.contains("review a coding agent's turn"))
        })
        .cloned()
        .collect()
}

#[tokio::test]
async fn a_judge_sure_the_turn_is_stuck_ends_it_with_what_was_tried() {
    let mut scripts = cycle(11);
    scripts.push(judge(
        r#"{"stuck": true, "confidence": 0.95, "redirect": "Ask the user which branch."}"#,
    ));
    let (provider, events, turn) = run("judge_stuck", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Failed);
    let error = turn.error_message.unwrap_or_default();
    assert!(error.contains("a reviewer found the turn stuck"), "{error}");
    assert!(
        error.contains(r#"- echo {"text":"git diff"} -> ok"#),
        "{error}"
    );
    assert!(
        error.contains("suggested: Ask the user which branch."),
        "{error}"
    );
    assert_eq!(notices(&events).last(), Some(&(RepeatRung::JudgeStop, 5)));
    let asked = judge_requests(&provider);
    assert_eq!(asked.len(), 1);
    let Some(ChatMessage::User(digest)) = asked[0].get(1) else {
        panic!("the digest is the judge's message");
    };
    assert!(digest.contains("tidy the repo"), "{digest}");
    assert!(digest.contains("came round 5 times"), "{digest}");
}

#[tokio::test]
async fn a_judge_not_sure_gives_advice_and_the_turn_goes_on() {
    let mut scripts = cycle(11);
    scripts.push(judge(
        r#"{"stuck": true, "confidence": 0.6, "redirect": "Commit, then look at the log."}"#,
    ));
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (provider, events, turn) = run("judge_redirect", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(notices(&events).contains(&(RepeatRung::JudgeRedirect, 5)));
    let requests = provider.requests.lock().unwrap().clone();
    // The step after the judge reads its advice last.
    let after = requests
        .iter()
        .find(|messages| {
            messages.iter().any(|message| {
                matches!(message, ChatMessage::System(note) if note.contains("suggests: Commit"))
            })
        })
        .expect("a step read the advice");
    assert!(matches!(after.last(), Some(ChatMessage::System(note)) if note.contains("reviewer")));
}

#[tokio::test]
async fn an_answer_that_cannot_be_read_lets_the_turn_go_on() {
    let mut scripts = cycle(11);
    scripts.push(judge("I think it is stuck."));
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (_, events, turn) = run("judge_unread", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    let rungs: Vec<RepeatRung> = notices(&events).into_iter().map(|n| n.0).collect();
    assert!(!rungs.contains(&RepeatRung::JudgeStop) && !rungs.contains(&RepeatRung::JudgeRedirect));
}

#[tokio::test]
async fn with_the_judge_off_no_judge_is_asked() {
    let mut scripts = cycle(11);
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let settings = "limits:\n  loop_judge: false\n";
    let (provider, _, turn) = run("judge_off", settings, scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(judge_requests(&provider).is_empty());
}

#[tokio::test]
async fn varied_work_and_batch_edits_are_never_judged() {
    // Forty steps, each a call with a new result: reads of new files, an edit each.
    let mut scripts = calls(40, |i| format!("edited src/module_{i}.rs"));
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (provider, events, turn) = run("judge_varied", "", scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(judge_requests(&provider).is_empty());
    assert_eq!(notices(&events), []);
}

/// How a helper server answers a chat request: status line, content type and body.
type Answer = fn(&str) -> (&'static str, &'static str, String);

/// Refuses each chat, so the turn goes on without a verdict.
fn refuse(_request: &str) -> (&'static str, &'static str, String) {
    (
        "400 Bad Request",
        "application/json",
        r#"{"error":{"message":"refused in the test"}}"#.to_owned(),
    )
}

/// A provider server on this machine that keeps every request it is sent and answers
/// each chat with `answer`.
async fn helper_server(answer: Answer) -> (String, Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut buffer = [0u8; 8192];
            // The head, then as much body as it announces.
            while let Ok(read) = socket.read(&mut buffer).await {
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                let Some(end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let (status, kind, body) = if text.starts_with("GET") {
                ("200 OK", "application/json", r#"{"data":[]}"#.to_owned())
            } else {
                answer(&text)
            };
            kept.lock().unwrap().push(text);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    (base, seen)
}

#[tokio::test]
async fn the_arbiters_helper_model_judges_when_the_user_set_one() {
    let (base, seen) = helper_server(refuse).await;
    let config = crate::testing::temp_live_config_of(
        "judge_helper_config",
        &format!(
            "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    \
             api_key: 'sk-test'\n  helper:\n    kind: openai_compatible\n    base_url: '{base}'\n    \
             api_key: 'sk-test'\narbiter:\n  model: helper/small-judge\n"
        ),
    );
    let mut scripts = cycle(11);
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (provider, _, turn) = run_with("judge_helper", config, scripts).await;
    // The helper refused, so the turn went on without a verdict.
    assert_eq!(turn.status, TurnStatus::Completed);
    assert!(judge_requests(&provider).is_empty());
    let seen = seen.lock().unwrap();
    let asked: Vec<&String> = seen.iter().filter(|r| r.starts_with("POST")).collect();
    assert!(!asked.is_empty(), "{seen:?}");
    assert!(asked.iter().all(|r| r.contains("small-judge")), "{asked:?}");
    assert!(
        asked.iter().all(|r| r.contains("review a coding agent")),
        "{asked:?}"
    );
}

#[tokio::test]
async fn a_helper_model_that_cannot_be_had_leaves_the_judge_to_the_turns_model() {
    let mut scripts = cycle(11);
    scripts.push(judge(
        r#"{"stuck": false, "confidence": 0.3, "redirect": "Commit, then look at the log."}"#,
    ));
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let settings = "arbiter:\n  model: nowhere/small-judge\n";
    let (provider, events, turn) = run("judge_helper_missing", settings, scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    assert_eq!(judge_requests(&provider).len(), 1);
    assert!(notices(&events).contains(&(RepeatRung::JudgeRedirect, 5)));
}

/// Refuses a request that carries a reasoning effort as a model without reasoning does,
/// and answers one without it with a verdict sure the turn is stuck.
fn no_reasoning(request: &str) -> (&'static str, &'static str, String) {
    no_reasoning_says(
        request,
        r#"{"stuck": true, "confidence": 0.95, "redirect": "Ask the user."}"#,
    )
}

/// The same, with a verdict that lets the turn go on.
fn no_reasoning_unsure(request: &str) -> (&'static str, &'static str, String) {
    no_reasoning_says(
        request,
        r#"{"stuck": false, "confidence": 0.4, "redirect": "Read the log first."}"#,
    )
}

fn no_reasoning_says(request: &str, verdict: &str) -> (&'static str, &'static str, String) {
    if request.contains("reasoning_effort") {
        return (
            "400 Bad Request",
            "application/json",
            concat!(
                r#"{"error":{"type":"invalid_request_error","code":"unsupported_parameter","#,
                r#""param":"reasoning_effort","message":"reasoning_effort is not supported"}}"#
            )
            .to_owned(),
        );
    }
    let verdict = serde_json::to_string(verdict).unwrap();
    let body = format!(
        "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{verdict}}}}}]}}\n\n\
         data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n\
         data: [DONE]\n\n"
    );
    ("200 OK", "text/event-stream", body)
}

#[tokio::test]
async fn a_helper_model_without_reasoning_is_asked_again_without_an_effort() {
    let (base, seen) = helper_server(no_reasoning).await;
    let config = crate::testing::temp_live_config_of(
        "judge_no_reasoning_config",
        &format!(
            "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    \
             api_key: 'sk-test'\n  helper:\n    kind: openai_compatible\n    base_url: '{base}'\n    \
             api_key: 'sk-test'\narbiter:\n  model: helper/small-judge\n"
        ),
    );
    let mut scripts = cycle(11);
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (_, events, turn) = run_with("judge_no_reasoning", config, scripts).await;
    // The second ask, without the effort, gave the verdict.
    assert_eq!(turn.status, TurnStatus::Failed);
    assert_eq!(notices(&events).last(), Some(&(RepeatRung::JudgeStop, 5)));
    let seen = seen.lock().unwrap();
    let asked: Vec<&String> = seen.iter().filter(|r| r.starts_with("POST")).collect();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].contains("reasoning_effort"), "{}", asked[0]);
    assert!(!asked[1].contains("reasoning_effort"), "{}", asked[1]);
}

#[tokio::test]
async fn a_helper_model_that_refused_the_effort_is_asked_without_one_from_then_on() {
    let (base, seen) = helper_server(no_reasoning_unsure).await;
    let config = crate::testing::temp_live_config_of(
        "judge_no_reasoning_again_config",
        &format!(
            "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    \
             api_key: 'sk-test'\n  helper:\n    kind: openai_compatible\n    base_url: '{base}'\n    \
             api_key: 'sk-test'\narbiter:\n  model: helper/small-judge\n"
        ),
    );
    let mut scripts = cycle(40);
    scripts.push(vec![Step::Text("done"), Step::Done]);
    let (_, _, turn) = run_with("judge_no_reasoning_again", config, scripts).await;
    assert_eq!(turn.status, TurnStatus::Completed);
    let seen = seen.lock().unwrap();
    let asked: Vec<&String> = seen.iter().filter(|r| r.starts_with("POST")).collect();
    // One refused ask, then each judge without the effort.
    assert!(asked.len() >= 3, "{asked:?}");
    assert!(asked[0].contains("reasoning_effort"), "{}", asked[0]);
    assert!(
        asked[1..].iter().all(|r| !r.contains("reasoning_effort")),
        "{asked:?}"
    );
}
