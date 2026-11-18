//! Command output sent shaped in running turns: what the next request carries, that the
//! whole output stays stored and reads back, and that with the switch off nothing
//! differs from a thread that never had it.

use super::*;
use crate::runtime::thread::Conversation;
use crate::state::{Message, estimate_tokens};
use crate::tools::{RunCommand, ToolResult};

const ON: &str = "limits:\n  shape_output: true\n";
const CARGO_TEST: &str = include_str!("../../../../tools/console/shape/fixtures/cargo_test.txt");
/// Output of each kind the shapers know, and one of no program at all.
const OUTPUTS: [&str; 6] = [
    CARGO_TEST,
    include_str!("../../../../tools/console/shape/fixtures/pytest_verbose.txt"),
    include_str!("../../../../tools/console/shape/fixtures/npm_test.txt"),
    include_str!("../../../../tools/console/shape/fixtures/git_push.txt"),
    "exit code: 0\nstdout:\nsame\nsame\nsame\n",
    "exit code: 1\nstderr:\n\u{1b}[31merror\u{1b}[0m: caf\u{e9} \u{fffd}\r\n",
];

/// Called as `run_command`: returns `output` as a command that exited with 101 would,
/// and shapes it as the real tool does.
struct Command {
    output: &'static str,
}

impl Tool for Command {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Runs a command.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn call<'a>(&'a self, _arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move { Ok(self.output.to_owned()) })
    }

    fn call_result<'a>(
        &'a self,
        _arguments: Value,
        _context: ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            Ok(ToolResult {
                content: self.output.to_owned(),
                is_error: true,
                exit_code: Some(101),
                artifacts: Vec::new(),
            })
        })
    }

    fn shape(&self, arguments: &Value, result: &ToolResult) -> Option<crate::tools::Shaped> {
        RunCommand::default().shape(arguments, result)
    }
}

/// One turn: `cargo test`, then an answer. The provider and the events.
async fn run_cargo_test(
    state: &Arc<State>,
    name: &str,
    settings: &str,
    output: &'static str,
) -> (Arc<FakeProvider>, Vec<ThreadEvent>) {
    let config = Arc::new(temp_live_config_with(&format!("{name}_config"), settings));
    let call = Step::Named(
        "run_command",
        "c1".into(),
        json!({ "command": "cargo test" }),
    );
    let provider = provider(vec![
        vec![call, Step::Done],
        vec![Step::Text("done"), Step::Done],
    ]);
    let tool = Box::new(Command { output });
    let (runner, input, mut output) = runner_with(state, &provider, &config, model(None), tool);
    input.send("test it").await.unwrap();
    drop(input);
    runner.run().await;
    (Arc::clone(&provider), drain(&mut output).await)
}

/// What the step after the call sent as the call's result.
fn sent_result(provider: &FakeProvider) -> String {
    let requests = provider.requests.lock().unwrap();
    let request = requests
        .iter()
        .find(|request| {
            request
                .iter()
                .any(|m| matches!(m, ChatMessage::Tool { .. }))
        })
        .expect("a request after the call");
    request
        .iter()
        .find_map(|message| match message {
            ChatMessage::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .unwrap()
}

async fn stored_result(state: &State) -> Message {
    let messages = state.list_messages("thread").await.unwrap();
    messages
        .into_iter()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap()
}

fn shaped_events(events: &[ThreadEvent]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ResultShaped { saved_tokens } => Some(*saved_tokens),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_model_is_sent_the_shaped_output_and_the_whole_stays_stored() {
    let state = thread_state("shaped_sent").await;
    let (provider, events) = run_cargo_test(&state, "shaped_sent", ON, CARGO_TEST).await;
    let stored = stored_result(&state).await;
    assert_eq!(
        stored.content, CARGO_TEST,
        "the stored result is the whole output"
    );

    let sent = sent_result(&provider);
    let note = format!(
        "\n[75 lines left out: passing tests, build progress. The whole output is stored: \
         read_history {{\"result\": {}}} returns it without running anything.]",
        stored.sequence
    );
    assert!(sent.ends_with(&note), "{sent}");
    assert!(
        sent.contains("test tests::fails_on_purpose ... FAILED"),
        "{sent}"
    );
    assert!(!sent.contains("test tests::adds ... ok"), "{sent}");
    let saved = stored.content_tokens - (estimate_tokens(&sent) + 4);
    assert_eq!(shaped_events(&events), [saved]);

    // A later load sends the same bytes: the text was decided once.
    let conversation = Conversation::load(&state, "thread", false).await.unwrap();
    let again = conversation.chat_messages();
    let tool = again.iter().find_map(|message| match message {
        ChatMessage::Tool { content, .. } => Some(content.clone()),
        _ => None,
    });
    assert_eq!(tool.as_deref(), Some(sent.as_str()));
}

/// `read_history {"result": N}`, which the note names, returns the whole output.
#[tokio::test]
async fn read_history_returns_the_whole_output_of_a_shaped_result() {
    let state = thread_state("shaped_read_back").await;
    run_cargo_test(&state, "shaped_read_back", ON, CARGO_TEST).await;
    let stored = stored_result(&state).await;

    let history = crate::tools::ReadHistory::new(Arc::clone(&state), "thread".into());
    let root = temp_dir("shaped_read_back_scope");
    std::fs::create_dir_all(&root).unwrap();
    let scope = ToolScope::new(&root, &root).unwrap();
    let context = crate::testing::tool_context_with(&scope, ON);
    let arguments = json!({ "result": stored.sequence, "limit": 400 });
    let page = history.call(arguments, context.clone()).await.unwrap();
    let (header, lines) = page.split_once('\n').unwrap();
    assert!(header.contains("of 140"), "{header}");
    assert_eq!(lines.trim_end(), CARGO_TEST.trim_end());
    let arguments = json!({ "result": stored.sequence, "search": "tests::adds ..." });
    let found = history.call(arguments, context).await.unwrap();
    assert!(found.contains("test tests::adds ... ok"), "{found}");
}

/// With the switch off the model gets every output exactly as stored, whatever it
/// holds, and nothing is written beside it.
#[tokio::test]
async fn with_the_switch_off_every_output_is_sent_byte_for_byte() {
    for (index, output) in OUTPUTS.into_iter().enumerate() {
        let name = format!("shaped_off_{index}");
        let state = thread_state(&name).await;
        let (provider, events) = run_cargo_test(&state, &name, "", output).await;
        assert_eq!(sent_result(&provider), output);
        assert_eq!(stored_result(&state).await.content, output);
        assert!(shaped_events(&events).is_empty());
        assert!(state.shaped_results("thread", 0).await.unwrap().is_empty());
    }
}

/// A shaped text is worth its note only when it saves enough; small output goes whole.
#[tokio::test]
async fn output_that_shapes_to_little_less_is_sent_whole() {
    let state = thread_state("shaped_small").await;
    let output = "exit code: 0\nstderr:\n   Compiling a v0.1.0\n    Finished `dev` profile\n";
    let (provider, events) = run_cargo_test(&state, "shaped_small", ON, output).await;
    assert_eq!(sent_result(&provider), output);
    assert!(shaped_events(&events).is_empty());
}
