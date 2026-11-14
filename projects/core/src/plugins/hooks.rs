//! Hooks: programs of plugins run when something happens in a thread. Each reads the
//! event as one JSON object on stdin, such as
//! `{"event": "before_tool", "thread": "…", "tool": "run_command", "arguments": {…}}`,
//! and may print one JSON object back:
//!
//! - `{"decision": "deny", "reason": "…"}` before a tool keeps it from running;
//! - `{"context": "…"}` adds a note the model reads: to the thread at the start of a turn
//!   or with a user message, to the tool's result after a tool.
//!
//! Printing nothing allows. A hook before a tool that fails or runs past its time denies
//! the call, since a check that could not run has not allowed anything; other hooks that
//! fail are logged and skipped.

use std::{process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

use super::{
    isolation::Isolation,
    manifest::{HookConfig, HookEvent},
};

/// Far above any decision; a hook that prints more is not one to trust.
const MAX_ANSWER_BYTES: u64 = 64 * 1024;

/// A hook of a plugin that is on, ready to run.
#[derive(Clone)]
pub struct Hook {
    pub plugin: String,
    pub config: HookConfig,
    /// The program and its arguments, `./` resolved to the plugin's folder.
    pub command: Vec<String>,
    pub isolation: Isolation,
}

/// The hooks of every plugin that is on; replaced whole when plugins change.
#[derive(Default)]
pub struct Hooks {
    hooks: RwLock<Arc<Vec<Hook>>>,
}

/// What the hooks of one event said.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Why a tool may not run; only hooks before a tool deny.
    pub denied: Option<String>,
    /// Notes for the model.
    pub context: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    decision: Option<Decision>,
    reason: Option<String>,
    context: Option<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Allow,
    Deny,
}

impl Hooks {
    pub fn set(&self, hooks: Vec<Hook>) {
        *self.hooks.write() = Arc::new(hooks);
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.read().is_empty()
    }

    /// Runs the hooks of `event`, one after another, in the order plugins are named.
    /// `tool` picks the hooks of a tool event; `event` carries its details.
    pub async fn run(&self, event: HookEvent, tool: Option<&str>, details: Value) -> Outcome {
        let hooks = Arc::clone(&self.hooks.read());
        let mut outcome = Outcome::default();
        let mut input = details;
        input["event"] = json!(name(event));
        for hook in hooks.iter().filter(|hook| applies(hook, event, tool)) {
            match run_one(hook, &input).await {
                Ok(answer) => {
                    if let Some(context) = answer.context.filter(|text| !text.trim().is_empty()) {
                        outcome.context.push(context);
                    }
                    if event == HookEvent::BeforeTool && answer.decision == Some(Decision::Deny) {
                        let reason = answer.reason.unwrap_or_else(|| "no reason given".into());
                        outcome.denied = Some(format!("plugin {} refused: {reason}", hook.plugin));
                        return outcome;
                    }
                }
                Err(error) if event == HookEvent::BeforeTool => {
                    outcome.denied =
                        Some(format!("plugin {}'s check failed: {error:#}", hook.plugin));
                    return outcome;
                }
                Err(error) => warn!("hook of plugin {} failed: {error:#}", hook.plugin),
            }
        }
        outcome
    }
}

fn applies(hook: &Hook, event: HookEvent, tool: Option<&str>) -> bool {
    hook.config.event == event
        && (hook.config.tools.is_empty()
            || tool.is_some_and(|tool| hook.config.tools.iter().any(|name| name == tool)))
}

async fn run_one(hook: &Hook, input: &Value) -> Result<Answer> {
    let program = hook.command.first().context("the hook has no command")?;
    let mut child = hook
        .isolation
        .command(&hook.command)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("cannot start {program}"))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let mut stdout = child.stdout.take().context("no stdout")?;
    let line = format!("{input}\n");
    let ran = async {
        // A hook that does not read its input must not hold the call up.
        let _ = stdin.write_all(line.as_bytes()).await;
        drop(stdin);
        let mut printed = Vec::new();
        (&mut stdout)
            .take(MAX_ANSWER_BYTES + 1)
            .read_to_end(&mut printed)
            .await?;
        let status = child.wait().await?;
        anyhow::Ok((status, printed))
    };
    let (status, printed) = tokio::time::timeout(Duration::from_secs(hook.config.timeout), ran)
        .await
        .map_err(|_| anyhow::anyhow!("it ran past {} seconds", hook.config.timeout))??;
    if !status.success() {
        bail!("it exited with {status}");
    }
    if printed.len() as u64 > MAX_ANSWER_BYTES {
        bail!("it printed more than {MAX_ANSWER_BYTES} bytes");
    }
    if printed.iter().all(u8::is_ascii_whitespace) {
        return Ok(Answer {
            decision: None,
            reason: None,
            context: None,
        });
    }
    serde_json::from_slice(&printed).context("it printed something other than a JSON answer")
}

fn name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::TurnStart => "turn_start",
        HookEvent::UserMessage => "user_message",
        HookEvent::BeforeTool => "before_tool",
        HookEvent::AfterTool => "after_tool",
        HookEvent::TurnEnd => "turn_end",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::manifest::HookConfig;

    fn isolation() -> Isolation {
        let paths = crate::testing::temp_paths("hook");
        let folder = paths.rules_root().join("plugins/test");
        let data = paths.config_root().join("plugin-data/test");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let permissions = Default::default();
        Isolation::new(&paths, &folder, &data, &permissions, Vec::new(), None).unwrap()
    }

    fn hook(event: HookEvent, script: &str) -> Hook {
        Hook {
            plugin: "test".into(),
            config: HookConfig {
                event,
                tools: vec!["run_command".into()],
                command: Vec::new(),
                timeout: 2,
            },
            command: vec!["/bin/sh".into(), "-c".into(), script.into()],
            isolation: isolation(),
        }
    }

    async fn before_tool(script: &str, tool: &str) -> Outcome {
        let hooks = Hooks::default();
        hooks.set(vec![hook(HookEvent::BeforeTool, script)]);
        hooks
            .run(HookEvent::BeforeTool, Some(tool), json!({ "tool": tool }))
            .await
    }

    #[tokio::test]
    async fn a_check_before_a_tool_denies_and_fails_closed() {
        let denied = before_tool(r#"echo '{"decision":"deny","reason":"no"}'"#, "run_command");
        assert_eq!(
            denied.await.denied.as_deref(),
            Some("plugin test refused: no")
        );
        assert!(
            before_tool("cat >/dev/null", "run_command")
                .await
                .denied
                .is_none()
        );
        // Only the tools it names.
        assert!(before_tool("exit 1", "read_file").await.denied.is_none());
        for broken in ["exit 1", "echo not json", "sleep 5"] {
            let outcome = before_tool(broken, "run_command").await;
            assert!(outcome.denied.is_some(), "{broken}");
        }
        let noted = before_tool(r#"echo '{"context":"careful"}'"#, "run_command").await;
        assert_eq!(noted.context, ["careful"]);
    }
}
