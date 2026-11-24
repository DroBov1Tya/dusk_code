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
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
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
