//! `read_history`: the thread's own messages, read by sequence or found by text. After a
//! compaction the summary stands in for older messages; this is how the agent gets
//! their details back. With `thread` it reads another of the user's chats the same way,
//! named by the link the user pasted.

use std::sync::Arc;

use anyhow::{Result, bail, ensure};
use dusk_protocol::text::clip_chars;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::stored_result;
use crate::tools::{
    schema,
    schema::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::{
    BoxFuture,
    state::{Message, MessageRole, State, calls_of},
};
use dusk_protocol::thread::{
    Part, THREAD_LINK_PREFIX, links_in, split, thread_link, thread_of_link,
};

/// Added to the description while the `result` form is offered.
const RESULT_SENTENCE: &str = " `result` N returns tool result #N whole, `limit` lines from line `offset`; `search` then finds lines in it.";
const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;
/// Keeps one long message from filling the whole result.
const MAX_MESSAGE_CHARS: usize = 2000;
/// How many messages naming a link are looked through for the user's own.
const LINK_SEARCH: u32 = 200;
/// Agents ask agents only this deep; a longer chain of parents is not followed.
const MAX_PARENTS: usize = 16;

pub struct ReadHistory {
    state: Arc<State>,
    thread_id: String,
    /// Offers `result` whatever the config says; for tests.
    result_form_forced: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadHistoryArguments {
    /// Text to look for.
    search: Option<String>,
    /// First message number to read. Defaults to 1.
    #[schemars(range(min = 1))]
    from: Option<u64>,
    /// How many messages (default 20, at most 50), or lines of a result (default 200, at
    /// most 400).
    #[schemars(range(min = 1, max = stored_result::MAX_LINES))]
    limit: Option<u32>,
    /// The number of a tool result to read in full, as shown by `#N`.
    #[schemars(range(min = 1))]
    result: Option<u64>,
    /// First line of the result to read. Defaults to 1.
    #[schemars(range(min = 1))]
    offset: Option<usize>,
    /// Another chat to read: its `dusk://thread/<id>` link or id. Defaults to this one.
    thread: Option<String>,
}

/// Whether the `result` form is offered and accepted: only where results are sent as
/// stubs or shaped, since a stub or a shaped result's note is what names one to read.
fn result_form_on(context: &ToolContext) -> bool {
    let limits = context.config.limits();
    limits.stub_results || limits.shape_output
}

impl ReadHistory {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self {
            state,
            thread_id,
            result_form_forced: false,
        }
    }

    #[cfg(test)]
    fn with_result_form(mut self) -> Self {
        self.result_form_forced = true;
        self
    }

    fn result_form(&self, context: &ToolContext) -> bool {
        self.result_form_forced || result_form_on(context)
    }

    /// The thread a call reads and the line that names it: this one, or another chat of
    /// the user's store by its link or id.
    async fn target(&self, named: Option<&str>) -> Result<(String, String)> {
        let Some(named) = named else {
            return Ok((self.thread_id.clone(), String::new()));
        };
        let id = thread_of_link(named).ok_or_else(|| {
            anyhow::anyhow!("`thread` takes a chat link `{THREAD_LINK_PREFIX}<id>` or a thread id")
        })?;
        if id == self.thread_id {
            return Ok((self.thread_id.clone(), String::new()));
        }
        ensure!(
            self.linked_by_user(id).await?,
            "chat {id} was not linked by the user in this chat: ask them to paste its link"
        );
        let thread =
            self.state.load_thread(id).await?.ok_or_else(|| {
                anyhow::anyhow!("no chat {id}: ask the user to copy its link again")
            })?;
        let title = thread.title.as_deref().unwrap_or("untitled");
        Ok((thread.id.clone(), format!("chat {title:?} ({id}):\n\n")))
    }

    /// Whether the user wrote the link to `id` in the chat this thread belongs to: in what
    /// they typed or pasted, never in a model's, a tool's or a file's text. An agent's own
    /// thread starts with its parent's words, so only the chat at the top counts.
    async fn linked_by_user(&self, id: &str) -> Result<bool> {
        let mut chat = self.thread_id.clone();
        let mut parents = 0;
        while let Some(parent) = self
            .state
            .load_thread(&chat)
            .await?
            .and_then(|thread| thread.parent_thread_id)
        {
            parents += 1;
            if parents > MAX_PARENTS {
                return Ok(false);
            }
            chat = parent;
        }
        let link = thread_link(id);
        let found = self
            .state
            .search_messages(&chat, &link, LINK_SEARCH)
            .await?;
        Ok(found.iter().any(|message| {
            message.role == MessageRole::User
                && split(&message.content).iter().any(|part| match part {
                    Part::Text(text) | Part::Pasted(text) => links_in(text).any(|it| it == id),
                    Part::Source(_) | Part::Sound { .. } => false,
                })
        }))
    }

    /// One stored tool result of `thread`. A number that is missing, another thread's or
    /// not a tool message all get the same answer.
    async fn read_result(
        &self,
        thread: &str,
        arguments: &ReadHistoryArguments,
        number: u64,
    ) -> Result<String> {
        ensure!(
            arguments.from.is_none(),
            "`result` reads one result; `from` lists messages. Give one of them"
        );
        let place = if thread == self.thread_id {
            "this thread".to_owned()
        } else {
            format!("thread {thread}")
        };
        let not_found = || anyhow::anyhow!("no tool result #{number} in {place}");
        let found = self.state.messages_from(thread, number, 1).await?;
        let message = found
            .into_iter()
            .find(|message| message.sequence == number && message.role == MessageRole::Tool)
            .ok_or_else(not_found)?;
        let tool = self.tool_of(thread, &message).await?;
        if let Some(query) = &arguments.search {
            ensure!(
                arguments.offset.is_none() && arguments.limit.is_none(),
                "`search` in a result lists matching lines; it takes no `offset` or `limit`"
            );
            ensure!(!query.trim().is_empty(), "search text is empty");
            return Ok(stored_result::search(
                number,
                &tool,
                &message.content,
                query,
            ));
        }
        let limit = arguments
            .limit
            .map_or(stored_result::DEFAULT_LINES, |n| n as usize);
        ensure!(
            (1..=stored_result::MAX_LINES).contains(&limit),
            "limit must be between 1 and {} lines for a result",
            stored_result::MAX_LINES
        );
        let offset = arguments.offset.unwrap_or(1);
        ensure!(offset >= 1, "offset counts from 1");
        Ok(stored_result::page(
            number,
            &tool,
            &message.content,
            offset,
            limit,
        ))
    }

    /// The name of the tool whose call this result answers: the call sits in an assistant
    /// message shortly before it, so the search widens only when it is not there.
    async fn tool_of(&self, thread: &str, result: &Message) -> Result<String> {
        if result.tool_call_id.is_none() {
            return Ok("tool".into());
        }
        let mut span = 16u64;
        loop {
            let from = result.sequence.saturating_sub(span).max(1);
            // Through the result itself, which is how `calls_of` pairs them.
            let before = self
                .state
                .messages_from(thread, from, span as u32 + 1)
                .await?;
            if let Some((_, call)) = calls_of(&before).get(result.message_id.as_str()) {
                return Ok(call.name.clone());
            }
            if from == 1 {
                return Ok("tool".into());
            }
            span *= 4;
        }
    }
}

impl Tool for ReadHistory {
    fn name(&self) -> &'static str {
        "read_history"
    }

    fn description(&self, context: &ToolContext) -> String {
        let mut text = "Read earlier messages of this conversation, including those replaced by a \
            summary. Give `search` to find messages containing some text (at least 3 \
            characters, best matches first), or `from` to read in order from a message \
            number. Each message is shown with its number, so you can read on around a match. \
            A `dusk://thread/<id>` link the user wrote names another chat: pass it as \
            `thread` to read that chat the same way."
            .to_owned();
        if self.result_form(context) {
            text.push_str(RESULT_SENTENCE);
        }
        text
    }

    /// The schema a call is checked against; `call` refuses `result` while it is off.
    fn parameters(&self) -> Value {
        schema::of::<ReadHistoryArguments>()
    }

    /// With the switch off the model sees the tool as it was before `result`.
    fn offered_parameters(&self, context: &ToolContext) -> Value {
        let mut schema = self.parameters();
        if !self.result_form(context)
            && let Some(properties) = schema["properties"].as_object_mut()
        {
            properties.remove("result");
            properties.remove("offset");
            properties.insert(
                "limit".into(),
                json!({
                    "description": "How many messages. Defaults to 20.",
                    "type": "integer", "minimum": 1, "maximum": MAX_LIMIT,
                }),
            );
        }
        schema
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: ReadHistoryArguments = parse_arguments(arguments)?;
            let (thread, header) = self.target(arguments.thread.as_deref()).await?;
            if let Some(number) = arguments.result {
                ensure!(
                    self.result_form(&context),
                    "`result` is not available: use `search` or `from`"
                );
                let result = self.read_result(&thread, &arguments, number).await?;
                return Ok(header + &result);
            }
            ensure!(
                arguments.offset.is_none(),
                "`offset` goes with `result`; use `from` to start at a message"
            );
            let limit = arguments.limit.unwrap_or(DEFAULT_LIMIT);
            ensure!(
                (1..=MAX_LIMIT).contains(&limit),
                "limit must be between 1 and {MAX_LIMIT}"
            );
            let messages = match (&arguments.search, arguments.from) {
                (Some(_), Some(_)) => bail!("give either search or from, not both"),
                (Some(text), None) => {
                    let found = self.state.search_messages(&thread, text, limit);
                    found.await?
                }
                (None, from) => {
                    let from = from.unwrap_or(1);
                    let read = self.state.messages_from(&thread, from, limit);
                    read.await?
                }
            };
            if messages.is_empty() {
                return Ok(header + "no messages");
            }
            let messages: Vec<_> = messages.iter().map(describe).collect();
            Ok(header + &messages.join("\n\n"))
        })
    }
}

fn describe(message: &Message) -> String {
    let author = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool result",
        MessageRole::Job => "background job result",
    };
    let mut text = format!(
        "#{} {author}:\n{}",
        message.sequence,
        clip(&message.content)
    );
    for call in &message.tool_calls {
        let arguments = clip(&call.arguments.to_string());
        text.push_str(&format!("\ncalls {} {arguments}", call.name));
    }
    text
}

fn clip(text: &str) -> String {
    clip_chars(text, MAX_MESSAGE_CHARS, "... [cut]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{providers::ToolCall, state::TurnAnswer};
    use crate::{
        testing::{temp_dir, temp_state, tool_context, tool_context_with},
        tools::ToolScope,
    };

    async fn history(name: &str) -> ReadHistory {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        for (uid, text) in [("1", "Deploy to staging"), ("2", "Fix src/main.rs please")] {
            state.add_user_message("thread", uid, text).await.unwrap();
        }
        let other = "main.rs of another thread";
        state.add_user_message("other", "3", other).await.unwrap();
        ReadHistory::new(Arc::new(state), "thread".into())
    }

    async fn read(tool: &ReadHistory, arguments: Value) -> anyhow::Result<String> {
        let directory = temp_dir("history_scope");
        std::fs::create_dir_all(&directory).unwrap();
        let scope = ToolScope::new(&directory, &directory).unwrap();
        tool.call(arguments, tool_context(&scope)).await
    }

    #[tokio::test]
    async fn finds_substrings_in_its_own_thread_only() {
        let tool = history("history_search").await;
        let found = read(&tool, json!({ "search": "main.rs" })).await.unwrap();
        assert_eq!(found, "#2 user:\nFix src/main.rs please");
        let found = read(&tool, json!({ "search": "STAGING" })).await.unwrap();
        assert_eq!(found, "#1 user:\nDeploy to staging");
    }

    #[tokio::test]
    async fn reads_another_chat_only_when_the_user_linked_it() {
        let tool = history("history_link").await;
        let state = Arc::clone(&tool.state);
        state.create_thread("secret", "test").await.unwrap();
        state
            .add_user_message("secret", "4", "private")
            .await
            .unwrap();
        let (other, secret) = (thread_link("other"), thread_link("secret"));
        // The user's words link `other`; `secret` is named only by a file and a model.
        let typed = format!("look at {other} too");
        state.add_user_message("thread", "5", &typed).await.unwrap();
        let attached = dusk_protocol::thread::source_tag(&secret);
        state
            .add_user_message("thread", "6", &attached)
            .await
            .unwrap();
        let ask = TurnAnswer {
            content: format!("I will read {secret}"),
            reasoning: None,
        };
        let pending = state.add_pending("thread", "go").await.unwrap();
        state.admit_pending("thread", &pending).await.unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap().turn_id;
        let call = ToolCall {
            id: "c".into(),
            name: "read_history".into(),
            arguments: json!({ "thread": secret }),
        };
        state
            .record_tool_calls(&turn, ask, vec![call])
            .await
            .unwrap();

        let found = read(&tool, json!({ "thread": other, "from": 1 })).await;
        let found = found.unwrap();
        assert!(
            found.ends_with("#1 user:\nmain.rs of another thread"),
            "{found}"
        );
        assert!(!found.contains("staging"));
        for (bad, says) in [
            ("dusk://thread/../other", "takes a chat link"),
            (secret.as_str(), "not linked by the user"),
            ("secret", "not linked by the user"),
            ("dusk://thread/missing", "not linked by the user"),
        ] {
            let error = read(&tool, json!({ "thread": bad })).await.unwrap_err();
            assert!(format!("{error:#}").contains(says), "{bad}: {error:#}");
        }
    }

    #[tokio::test]
    async fn query_syntax_is_plain_text() {
        let tool = history("history_syntax").await;
        let found = read(&tool, json!({ "search": "\"main OR" })).await.unwrap();
        assert_eq!(found, "no messages");
        assert!(read(&tool, json!({ "search": "ma" })).await.is_err());
    }
    /// What the model was shown before `result` existed.
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Before {
        /// Text to look for.
        search: Option<String>,
        /// First message number to read. Defaults to 1.
        #[schemars(range(min = 1))]
        from: Option<u64>,
        /// How many messages. Defaults to 20.
        #[schemars(range(min = 1, max = MAX_LIMIT))]
        limit: Option<u32>,
        /// Another chat to read: its `dusk://thread/<id>` link or id. Defaults to this one.
        thread: Option<String>,
    }

    /// The tool as the model sees it with the `result` form on; the thread `thread` has a
    /// `run_command` result of `lines` lines as message `#3`, and `other` holds one too.
    async fn with_results(name: &str, lines: usize) -> ReadHistory {
        let state = temp_state(name).await;
        for thread in ["thread", "other"] {
            state.create_thread(thread, "test").await.unwrap();
            let pending = state.add_pending(thread, "go").await.unwrap();
            state.admit_pending(thread, &pending).await.unwrap();
            let turn = state.start_turn(thread).await.unwrap().unwrap().turn_id;
            let call = ToolCall {
                id: "c".into(),
                name: "run_command".into(),
                arguments: json!({ "command": "make" }),
            };
            let answer = TurnAnswer {
                content: String::new(),
                reasoning: None,
            };
            let calls = vec![call];
            state.record_tool_calls(&turn, answer, calls).await.unwrap();
            let text: String = (1..=lines).map(|n| format!("line {n}\n")).collect();
            state.record_tool_result(&turn, "c", text).await.unwrap();
        }
        ReadHistory::new(Arc::new(state), "thread".into()).with_result_form()
    }

    #[tokio::test]
    async fn a_result_is_paged_with_its_header() {
        let tool = with_results("history_pages", 450).await;
        let page = read(&tool, json!({ "result": 3 })).await.unwrap();
        let expected = "#3 run_command result, lines 1-200 of 450; next offset 201\nline 1\n";
        assert!(page.starts_with(expected), "{page}");
        let page = read(&tool, json!({ "result": 3, "offset": 401, "limit": 400 }));
        let page = page.await.unwrap();
        assert!(page.starts_with("#3 run_command result, lines 401-450 of 450\nline 401\n"));
        assert!(!page.contains("next offset"));
        let big = read(&tool, json!({ "result": 3, "limit": 401 })).await;
        assert!(big.is_err());
    }

    #[tokio::test]
    async fn a_result_is_searched_with_context_and_a_cap() {
        let tool = with_results("history_search_result", 450).await;
        let found = read(&tool, json!({ "result": 3, "search": "line 100" })).await;
        let found = found.unwrap();
        assert!(found.starts_with("#3 run_command result, 450 lines; 1 matching\n98- line 98\n"));
        assert!(found.contains("100: line 100\n101- line 101\n102- line 102\n"));
        let many = read(&tool, json!({ "result": 3, "search": "line" }))
            .await
            .unwrap();
        let marked = many.lines().filter(|line| line.contains(": line "));
        assert_eq!(marked.count(), 50, "{many}");
        assert!(many.contains("450 matching") && many.contains("400 more matches not shown"));
    }

    #[tokio::test]
    async fn only_a_tool_message_of_this_thread_is_a_result() {
        let tool = with_results("history_foreign", 5).await;
        // #1 is the user's message, #2 the assistant's, #3 the result; the other thread's
        // numbers overlap, so ask for one that exists only there.
        let state = tool.state.clone();
        let extra = state
            .add_user_message("other", "u", "only there")
            .await
            .unwrap();
        let mut errors = Vec::new();
        for number in [1, 2, 99, extra.sequence + 50] {
            let error = read(&tool, json!({ "result": number })).await.unwrap_err();
            errors.push(format!("{error:#}"));
        }
        assert!(
            errors[0].contains("no tool result #1 in this thread"),
            "{errors:?}"
        );
        assert!(errors[1].contains("no tool result #2"));
        assert!(errors[2].contains("no tool result #99"));
        // The other thread's own tool message looks the same as a missing one.
        let theirs = state.messages_from("other", 1, 10).await.unwrap();
        let theirs = theirs
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Tool)
            .unwrap();
        let tool_here = ReadHistory::new(state.clone(), "empty".into()).with_result_form();
        let error = read(&tool_here, json!({ "result": theirs.sequence })).await;
        assert_eq!(
            format!("{:#}", error.unwrap_err()),
            format!("no tool result #{} in this thread", theirs.sequence)
        );
    }

    #[tokio::test]
    async fn result_does_not_mix_with_the_message_forms() {
        let tool = with_results("history_mix", 5).await;
        for arguments in [
            json!({ "result": 3, "from": 1 }),
            json!({ "result": 3, "search": "line", "offset": 2 }),
            json!({ "result": 3, "search": "line", "limit": 5 }),
            json!({ "offset": 2 }),
        ] {
            assert!(read(&tool, arguments.clone()).await.is_err(), "{arguments}");
        }
    }

    #[tokio::test]
    async fn the_spec_is_unchanged_while_the_form_is_off() {
        let tool = with_results("history_spec", 5).await;
        let directory = temp_dir("history_spec_scope");
        std::fs::create_dir_all(&directory).unwrap();
        let scope = ToolScope::new(&directory, &directory).unwrap();
        let off = tool_context_with(
            &scope,
            "limits:\n  stub_results: false\n  shape_output: false\n",
        );
        let plain = ReadHistory::new(tool.state.clone(), "thread".into());
        assert_eq!(plain.offered_parameters(&off), schema::of::<Before>());
        let before = "Read earlier messages of this conversation, including those replaced by a \
            summary. Give `search` to find messages containing some text (at least 3 \
            characters, best matches first), or `from` to read in order from a message \
            number. Each message is shown with its number, so you can read on around a match. \
            A `dusk://thread/<id>` link the user wrote names another chat: pass it as \
            `thread` to read that chat the same way.";
        assert_eq!(plain.description(&off), before);
        let offered = tool.offered_parameters(&off);
        assert!(offered["properties"].get("result").is_some());
        assert!(tool.description(&off).starts_with(before));
        assert!(tool.description(&off).len() - before.len() < 120);
        let error = plain
            .call(
                json!({ "result": 3 }),
                tool_context_with(
                    &scope,
                    "limits:\n  stub_results: false\n  shape_output: false\n",
                ),
            )
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("not available"));
        assert!(
            tool.repeatable(&json!({ "result": 3 })) && tool.read_only(&json!({ "result": 3 }))
        );
    }
}
