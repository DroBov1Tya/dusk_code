//! The user's auto-compaction setting: the share of the input limit a running turn
//! compacts at (`limits.auto_compact_percent`), and the switch that leaves compaction to
//! `/compact` alone (`limits.auto_compact`).

use super::stubs::Command;
use super::*;
use dusk_protocol::thread::{AUTO_COMPACT_OFF_HINT, Note, note};

/// No early compaction, no tail and no stubs, so only the setting under test decides.
const PLAIN: &str = "limits:\n  compact_early: false\n  compact_tail: false\n  shrink_context: false\n  stub_results: false\n";

/// A step that prints 350 lines of 20 tokens: about 7,000 tokens of result.
fn big_call() -> Vec<Step> {
    let arguments = json!({ "command": "print", "lines": 350 });
    vec![
        Step::Named("run_command", "call_1".into(), arguments),
        Step::Done,
    ]
}

fn answered(text: &'static str) -> Vec<Step> {
    vec![Step::Text(text), Step::Done]
}

async fn run(name: &str, settings: &str, window: u64, scripts: Vec<Vec<Step>>) -> Vec<ThreadEvent> {
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let provider = provider(scripts);
    let tool = Box::new(Command);
    let (runner, input, mut output) =
        runner_with(&state, &provider, &config, model(Some(window)), tool);
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;
    drain(&mut output).await
}

fn compactions(events: &[ThreadEvent]) -> usize {
    let started = |e: &&ThreadEvent| matches!(e, ThreadEvent::CompactionStarted);
    events.iter().filter(started).count()
}

/// A result that leaves the context at about 70% of the window compacts within the turn
/// at a threshold of 65%, and not at 80%.
#[tokio::test]
async fn a_turn_compacts_at_the_threshold_set() {
    for (percent, compacted) in [(65, 1), (80, 0)] {
        let settings = format!("{PLAIN}  auto_compact_percent: {percent}\n");
        let scripts = match compacted {
            1 => vec![big_call(), answered("summary"), answered("done")],
            _ => vec![big_call(), answered("done")],
        };
        let name = format!("auto_compact_at_{percent}");
        let events = run(&name, &settings, 10_000, scripts).await;
        assert_eq!(compactions(&events), compacted, "at {percent}%");
        let failed = |e: &ThreadEvent| matches!(e, ThreadEvent::TurnFailed(_));
        assert!(!events.iter().any(failed), "at {percent}%");
    }
}

/// Off, a context that outgrows the window is never compacted: the turn fails and says
/// how to go on. On, the same turn compacts.
#[tokio::test]
async fn off_a_full_context_fails_the_turn_with_a_hint() {
    let off = format!("{PLAIN}  auto_compact: false\n");
    let events = run(
        "auto_compact_off",
        &off,
        5_000,
        vec![big_call(), answered("never")],
    )
    .await;
    assert_eq!(compactions(&events), 0);
    let failure = events
        .iter()
        .find_map(|e| match e {
            ThreadEvent::TurnFailed(failure) => Some(failure.error.clone()),
            _ => None,
        })
        .expect("the turn failed");
    assert!(failure.contains(AUTO_COMPACT_OFF_HINT), "{failure}");
    let full = events
        .iter()
        .find(|e| matches!(e, ThreadEvent::ContextTooLarge { .. }))
        .expect("the client is told");
    assert!(matches!(
        full,
        ThreadEvent::ContextTooLarge {
            auto_compact_off: true,
            ..
        }
    ));
    match note(full) {
        Some(Note::Error(text)) => assert!(text.contains(AUTO_COMPACT_OFF_HINT), "{text}"),
        other => panic!("not an error note: {other:?}"),
    }

    let on = run(
        "auto_compact_on",
        PLAIN,
        5_000,
        vec![big_call(), answered("summary"), answered("done")],
    )
    .await;
    assert!(compactions(&on) >= 1);
}
