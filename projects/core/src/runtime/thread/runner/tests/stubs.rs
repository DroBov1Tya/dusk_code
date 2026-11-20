//! Old results put away in running turns: when a move happens, what the next request
//! sends, and that a restart, the repeat note and the failure ladder see what they saw.

use std::sync::atomic::{AtomicUsize, Ordering};

use dusk_protocol::thread::RepeatRung;

use super::*;
use crate::providers::Usage;
use crate::state::{CallPurpose, Message, ModelCall};

/// Two steps kept whole, so short scripts reach a move.
const ON: &str = "limits:\n  stub_results: true\n  stub_keep_steps: 2\n  stub_min_tokens: 50\n";
const KEEP_4: &str = "limits:\n  stub_results: true\n  stub_keep_steps: 4\n  stub_min_tokens: 50\n";
/// The scripted model under a name of the Anthropic family.
const ANTHROPIC: &str =
    "  fake:\n    kind: anthropic\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";

/// Called as `run_command`: prints `lines` lines of 80 bytes (20 tokens), then `last`;
/// with `fail`, as an error.
pub(super) struct Command;

impl Tool for Command {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Prints lines.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let lines = arguments["lines"].as_u64().unwrap_or(1);
            let mut text: String = (1..=lines)
                .map(|n| format!("{n:>6} {}\n", "o".repeat(72)))
                .collect();
            text.push_str(arguments["last"].as_str().unwrap_or("done"));
            match arguments["fail"].as_bool() {
                Some(true) => Err(anyhow::anyhow!("{text}")),
                _ => Ok(text),
            }
        })
    }
}

/// A step that runs `command`, printing `lines` lines.
fn command(command: &str, lines: u64) -> Vec<Step> {
    command_with(json!({ "command": command, "lines": lines }))
}

/// Each call has its own id, as ids must differ within a turn.
fn command_with(arguments: Value) -> Vec<Step> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("call_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    vec![Step::Named("run_command", id, arguments), Step::Done]
}

fn answer() -> Vec<Step> {
    vec![Step::Text("done"), Step::Done]
}

/// Runs one turn of the chat on `scripts`; the provider and the events.
async fn run_on(
    state: &Arc<State>,
    name: &str,
    settings: &str,
    scripts: Vec<Vec<Step>>,
) -> (Arc<FakeProvider>, Vec<ThreadEvent>) {
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let (runner, input, mut output) =
        runner_with(state, &provider, &config, model(None), Box::new(Command));
    input.send("build it").await.unwrap();
    drop(input);
    runner.run().await;
    (Arc::clone(&provider), drain(&mut output).await)
}

/// The moves of a run, as `(results, saved_tokens)`.
fn moves(events: &[ThreadEvent]) -> Vec<(u32, u64)> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ResultsStubbed {
                results,
                saved_tokens,
            } => Some((*results, *saved_tokens)),
            _ => None,
        })
        .collect()
}

/// The `index`th request's tool results as sent.
fn sent_results(provider: &FakeProvider, index: usize) -> Vec<String> {
    provider.requests.lock().unwrap()[index]
        .iter()
        .filter_map(|message| match message {
            ChatMessage::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// The thread's stored tool results, in order.
async fn stored_results(state: &State) -> Vec<Message> {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .into_iter()
        .filter(|message| message.role == MessageRole::Tool)
        .collect()
}

/// The thread's stubs by the sequence of their result.
async fn stubbed(state: &State) -> Vec<u64> {
    let stubs = state.stubs("thread", 0).await.unwrap();
    let mut sequences: Vec<u64> = stored_results(state)
        .await
        .into_iter()
        .filter(|result| stubs.contains_key(&result.message_id))
        .map(|result| result.sequence)
        .collect();
    sequences.sort_unstable();
    sequences
}

/// A model call stored long enough ago that the provider's cache is gone.
async fn called_long_ago(state: &State) {
    let call = ModelCall {
        thread_id: "thread".into(),
        turn_id: None,
        purpose: CallPurpose::Title,
        compaction_id: None,
        model: "fake".into(),
        through_sequence: 1,
    };
    state.record_usage(&call, Usage::default()).await.unwrap();
    sqlx::query("UPDATE model_usage SET created_at = created_at - 400000")
        .execute(&state.pool)
        .await
        .unwrap();
}

/// A result of 5,000 tokens, then three small ones: from the fourth step the first is
/// two steps old and frees enough to pay for the cache it breaks.
fn big_then_small() -> Vec<Vec<Step>> {
    vec![
        command("make 0", 250),
        command("make 1", 15),
        command("make 2", 15),
        command("make 3", 15),
        answer(),
    ]
}

#[tokio::test]
async fn a_warm_move_puts_an_old_result_away_once() {
    let state = thread_state("stubs_warm").await;
    let (provider, events) = run_on(&state, "stubs_warm", ON, big_then_small()).await;

    let results = stored_results(&state).await;
    let stubs = state.stubs("thread", 0).await.unwrap();
    let big = &results[0];
    let stub = &stubs[&big.message_id];
    assert_eq!(moves(&events), [(1, stub.saved_tokens)]);
    assert_eq!(stub.made_at_sequence, 8, "the newest message when it moved");
    assert!(
        stub.text.starts_with(&format!(
            "[result {} put away: run_command \"make 0\" succeeded, 251 lines, {} tokens.",
            big.sequence, big.content_tokens
        )),
        "{}",
        stub.text
    );
    // The third request still sends it whole; the fourth sends the stub and the rest
    // whole; the fifth moves nothing more and extends the fourth.
    assert_eq!(sent_results(&provider, 2)[0], big.content);
    let fourth = sent_results(&provider, 3);
    assert_eq!(fourth[0], stub.text);
    assert_eq!(
        fourth[1..],
        [results[1].content.clone(), results[2].content.clone()]
    );
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(requests[4][..requests[3].len()], requests[3]);
}

/// `read_history {"result": N}` reads what the stub stands in for.
#[tokio::test]
async fn read_history_returns_a_stubbed_result_whole() {
    let state = thread_state("stubs_read_back").await;
    run_on(&state, "stubs_read_back", ON, big_then_small()).await;
    let big = stored_results(&state).await.remove(0);
    assert_eq!(stubbed(&state).await, [big.sequence]);

    let history = crate::tools::ReadHistory::new(Arc::clone(&state), "thread".into());
    let root = temp_dir("stubs_read_back_scope");
    std::fs::create_dir_all(&root).unwrap();
    let scope = ToolScope::new(&root, &root).unwrap();
    let context = crate::testing::tool_context_with(&scope, ON);
    let mut whole = String::new();
    let mut offset = 1;
    while offset <= 251 {
        let arguments = json!({ "result": big.sequence, "offset": offset, "limit": 50 });
        let page = history.call(arguments, context.clone()).await.unwrap();
        let (header, lines) = page.split_once('\n').unwrap();
        assert!(header.contains("of 251"), "{header}");
        whole.push_str(lines);
        offset += 50;
    }
    assert_eq!(whole.trim_end(), big.content);
}

#[tokio::test]
async fn below_the_thresholds_nothing_moves() {
    // 3,000 tokens are too few to free.
    let state = thread_state("stubs_too_few").await;
    let mut scripts = big_then_small();
    scripts[0] = command("make 0", 150);
    let (_, events) = run_on(&state, "stubs_too_few", ON, scripts).await;
    assert_eq!(moves(&events), []);
    assert!(stubbed(&state).await.is_empty());

    // 4,500 tokens freed, but under 0.6 of the 7,800 that would follow on a warm cache.
    let state = thread_state("stubs_too_small_share").await;
    let scripts = vec![
        command("make 0", 225),
        command("make 1", 195),
        command("make 2", 195),
        answer(),
    ];
    let (_, events) = run_on(&state, "stubs_too_small_share", ON, scripts).await;
    assert_eq!(moves(&events), []);
    assert!(stubbed(&state).await.is_empty());
}

/// The same turn as the share case above, after a model call over 300 s old.
#[tokio::test]
async fn a_cold_cache_moves_without_the_share() {
    let state = thread_state("stubs_cold").await;
    called_long_ago(&state).await;
    let scripts = vec![
        command("make 0", 225),
        command("make 1", 195),
        command("make 2", 195),
        answer(),
    ];
    let (provider, events) = run_on(&state, "stubs_cold", ON, scripts).await;
    let big = stored_results(&state).await.remove(0);
    assert_eq!(moves(&events).len(), 1);
    assert_eq!(stubbed(&state).await, [big.sequence]);
    assert!(sent_results(&provider, 3)[0].starts_with("[result "));
}

#[tokio::test]
async fn an_anthropic_provider_moves_only_on_a_cold_cache() {
    let settings = format!("{ANTHROPIC}{ON}");
    let state = thread_state("stubs_anthropic_warm").await;
    let (_, events) = run_on(&state, "stubs_anthropic_warm", &settings, big_then_small()).await;
    assert_eq!(moves(&events), []);
    assert!(stubbed(&state).await.is_empty());

    let state = thread_state("stubs_anthropic_cold").await;
    called_long_ago(&state).await;
    let (_, events) = run_on(&state, "stubs_anthropic_cold", &settings, big_then_small()).await;
    assert_eq!(moves(&events).len(), 1);
}

/// A move never changes a message at or before the newest stub, though one there has
/// grown old since.
#[tokio::test]
async fn a_move_only_touches_results_after_the_newest_stub() {
    let state = thread_state("stubs_frontier").await;
    let scripts = vec![
        command("make small", 15),
        command("make big", 250),
        command("make bigger", 250),
        command("make 3", 15),
        command("make 4", 15),
        command("make 5", 15),
        command("make 6", 15),
        answer(),
    ];
    let (provider, events) = run_on(&state, "stubs_frontier", KEEP_4, scripts).await;

    let results = stored_results(&state).await;
    // The big ones go two steps after they came, one move each; the small first one is
    // old from the sixth step, but it is before the newest stub by then.
    assert_eq!(moves(&events).len(), 2);
    assert_eq!(
        stubbed(&state).await,
        [results[1].sequence, results[2].sequence]
    );
    let last = sent_results(&provider, 7);
    assert_eq!(last[0], results[0].content);
}

#[tokio::test]
async fn a_reopened_thread_sends_the_same_prefix() {
    let state = thread_state("stubs_restart").await;
    let (before, _) = run_on(&state, "stubs_restart", ON, big_then_small()).await;
    let (after, events) = run_on(&state, "stubs_restart_again", ON, vec![answer()]).await;

    assert_eq!(moves(&events), []);
    let last = before.requests.lock().unwrap()[4].clone();
    let first = after.requests.lock().unwrap()[0].clone();
    assert_eq!(first[..last.len()], last);
}

#[tokio::test]
async fn with_the_switch_off_every_result_goes_whole() {
    let state = thread_state("stubs_off").await;
    let settings = "limits:\n  stub_results: false\n  stub_keep_steps: 2\n  stub_min_tokens: 50\n";
    let (provider, events) = run_on(&state, "stubs_off", settings, big_then_small()).await;

    assert_eq!(moves(&events), []);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM result_stubs")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    let stored: Vec<String> = stored_results(&state)
        .await
        .into_iter()
        .map(|result| result.content)
        .collect();
    assert_eq!(sent_results(&provider, 4), stored);
}

/// Stubs change what the model reads of old results; the repeat note hashes each fresh
/// one as it comes, so it fires as it always did.
#[tokio::test]
async fn the_repeat_note_fires_as_before_while_results_move() {
    let state = thread_state("stubs_same_call").await;
    let mut scripts: Vec<_> = (0..5).map(|_| command("make", 150)).collect();
    scripts.push(answer());
    let (_, events) = run_on(&state, "stubs_same_call", ON, scripts).await;

    assert_eq!(moves(&events).len(), 1);
    let notices: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect();
    use RepeatRung::SameCall;
    assert_eq!(notices, [(SameCall, 3), (SameCall, 4), (SameCall, 5)]);
}

#[tokio::test]
async fn the_failure_ladder_climbs_as_before_while_results_move() {
    let state = thread_state("stubs_ladder").await;
    let mut scripts: Vec<_> = (1..=8)
        .map(|attempt| {
            command_with(json!({
                "command": format!("cat /tmp/try{attempt}/config.yml"),
                "lines": 150,
                "fail": true,
                "last": format!(
                    "cannot open /tmp/try{attempt}/config.yml: No such file or directory"
                ),
            }))
        })
        .collect();
    scripts.push(answer());
    let settings = format!("effort: high\n{ON}  failure_ladder: true\n");
    let (_, events) = run_on(&state, "stubs_ladder", &settings, scripts).await;

    assert!(!moves(&events).is_empty());
    let notices: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::RepeatNotice { rung, count } => Some((*rung, *count)),
            _ => None,
        })
        .collect();
    use RepeatRung::{Replan, SameError, Stop};
    let expected = [
        (SameError, 3),
        (SameError, 4),
        (Replan, 5),
        (Replan, 6),
        (Replan, 7),
        (Stop, 8),
    ];
    assert_eq!(notices, expected);
}

/// After a compaction the context is read cold: every old result goes at once, with no
/// threshold, and the switch off still means nothing.
#[tokio::test]
async fn putting_all_away_skips_the_thresholds_but_not_the_switch() {
    let state = thread_state("stubs_all").await;
    let mut scripts = big_then_small();
    scripts[0] = command("make 0", 150);
    let (_, events) = run_on(&state, "stubs_all", ON, scripts).await;
    assert_eq!(moves(&events), []);

    let config = Arc::new(temp_live_config_with("stubs_all_again", ON));
    let provider = provider(Vec::new());
    let (runner, _input, mut output) =
        runner_with(&state, &provider, &config, model(None), Box::new(Command));
    let load = crate::runtime::thread::Conversation::load(&state, "thread", true);
    let mut conversation = load.await.unwrap();
    let off = crate::config::Limits::default();
    let agent = &runner.agent;
    agent
        .put_away_all_results(&mut conversation, &off)
        .await
        .unwrap();
    assert!(stubbed(&state).await.is_empty());

    let on = config.current().limits();
    agent
        .put_away_all_results(&mut conversation, &on)
        .await
        .unwrap();
    let results = stored_results(&state).await;
    // Results of the steps before the newest two: the first two of four.
    let old = [results[0].sequence, results[1].sequence];
    assert_eq!(stubbed(&state).await, old);
    assert_eq!(conversation.stubs.len(), 2);
    drop(runner);
    let events = drain(&mut output).await;
    assert_eq!(moves(&events).len(), 1);
    assert_eq!(moves(&events)[0].0, 2);
}
