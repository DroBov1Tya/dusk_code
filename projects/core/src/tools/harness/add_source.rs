//! `add_workshop_source`: the agent proposes a Workshop source the user asked it to find,
//! and the owner decides. It is asked every time, in every mode, full access included,
//! and only a client that could add the source itself may answer; the yes adds it the way
//! Settings does (`store::add`), and the list is read again then.

use std::sync::Arc;

use anyhow::Context;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    config::live::LiveConfig,
    plugins::store::add,
    tools::{
        approvals::Request,
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext},
    },
};

pub struct AddWorkshopSource {
    config: Arc<LiveConfig>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddSourceArguments {
    /// a-z, 0-9 and -, at most 32; shown as the listings' source.
    name: String,
    /// The https address of its index.json, or https://github.com/<owner>/<repository>.
    url: String,
    /// Base64 of its Ed25519 public key, when its publisher gives one; without it the
    /// source is Unverified.
    #[serde(default)]
    key: Option<String>,
}

impl AddWorkshopSource {
    pub fn new(config: Arc<LiveConfig>) -> Self {
        Self { config }
    }
}

impl Tool for AddWorkshopSource {
    fn name(&self) -> &'static str {
        "add_workshop_source"
    }

    /// A chat proposes it; its agents do not.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Propose adding a catalogue of skills, packs and agents to the Workshop. Use only \
         when the user asks to connect a catalogue; find its list's address first. The \
         user is asked and decides; without a key the source is Unverified and brings only \
         text pinned to a GitHub commit, never programs."
            .to_owned()
    }

    fn parameters(&self) -> Value {
        schema::of::<AddSourceArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: AddSourceArguments = parse_arguments(arguments)?;
            let key = arguments.key.unwrap_or_default();
            let mirror = add::proposed(&self.config, &arguments.url, &key, Some(&arguments.name))?;
            let request = Request::Settings {
                change: add::question(&mirror),
            };
            context.ask_owner(request).await?;
            let name = mirror.name.clone();
            let index = add::add(&self.config, mirror)
                .await
                .with_context(|| format!("{name} was not added"))?;
            let mut said = format!("Added {name}: {} listings read.", index.plugins.len());
            if let Some(note) = index.left_out {
                said.push_str(&format!(" {note}."));
            }
            Ok(said)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_dir, temp_live_config, tool_context};
    use crate::tools::{Approvals, ToolScope};

    fn call(tool: &AddWorkshopSource, url: &str, context: ToolContext) -> Result<String, String> {
        let arguments = serde_json::json!({"name": "far", "url": url});
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(tool.call(arguments, context))
            .map_err(|error| format!("{error:#}"))
    }

    #[test]
    fn nothing_is_added_without_the_owner_s_yes() {
        let config = Arc::new(temp_live_config("add-source"));
        let tool = AddWorkshopSource::new(Arc::clone(&config));
        let dir = temp_dir("add-source-scope");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let url = "https://far.example.com/index.json";
        // Nobody to ask, then nobody to answer: refused either way, whatever the mode.
        let error = call(&tool, url, tool_context(&scope)).unwrap_err();
        assert!(error.contains("owner's yes"), "{error}");
        let mut context = tool_context(&scope);
        context.approvals = Some(Arc::new(Approvals::default()));
        let error = call(&tool, url, context).unwrap_err();
        assert!(error.contains("owner's yes"), "{error}");
        assert!(config.current().workshop().sources.is_empty());
    }

    /// Where no owner can answer, the call is refused at once instead of waiting for
    /// good: a scheduled run, a headless run, a chat an API token started.
    #[tokio::test]
    async fn a_chat_nobody_can_answer_is_refused_at_once() {
        let config = Arc::new(temp_live_config("add-source-unattended"));
        let tool = AddWorkshopSource::new(Arc::clone(&config));
        let dir = temp_dir("add-source-unattended");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let state = Arc::new(crate::testing::temp_state("add-source-unattended").await);
        state.create_thread("nightly", "schedule").await.unwrap();
        state.create_thread("piped", "cli").await.unwrap();
        state.create_thread("api", "tui").await.unwrap();
        state.set_thread_owner("api", "token-1").await.unwrap();
        state.create_thread("mine", "tui").await.unwrap();
        let (events, _asked) = tokio::sync::mpsc::unbounded_channel();
        let context = |thread: &str| {
            let mut context = tool_context(&scope);
            context.approvals = Some(Arc::new(Approvals::default()));
            context.state = Some(Arc::clone(&state));
            context.thread_id = thread.to_owned();
            context.progress = Some(crate::tools::Progress {
                call_id: "c".into(),
                events: events.clone(),
            });
            context
        };
        let arguments =
            serde_json::json!({"name": "far", "url": "https://far.example.com/index.json"});
        for thread in ["nightly", "piped", "api"] {
            let call = tool.call(arguments.clone(), context(thread));
            let answer = tokio::time::timeout(std::time::Duration::from_secs(5), call)
                .await
                .expect("refused at once, not left waiting");
            let error = format!("{:#}", answer.unwrap_err());
            assert!(
                error.contains("nobody who may give it"),
                "{thread}: {error}"
            );
        }
        // The owner's own chat is asked, and waits for the answer.
        let call = tool.call(arguments, context("mine"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), call)
                .await
                .is_err()
        );
        assert!(config.current().workshop().sources.is_empty());
    }

    #[test]
    fn an_address_the_store_would_not_fetch_is_refused_before_asking() {
        let config = temp_live_config("add-source-url");
        for url in [
            "http://far.example.com/index.json",
            "https://127.0.0.1/index.json",
            "https://10.0.0.1/index.json",
            "https://[::1]/index.json",
            "https://user:pass@far.example.com/index.json",
            "file:///etc/passwd",
        ] {
            assert!(
                add::proposed(&config, url, "", Some("far")).is_err(),
                "{url}"
            );
        }
        let mirror = add::proposed(
            &config,
            "https://far.example.com/index.json",
            "",
            Some("far"),
        )
        .unwrap();
        assert!(mirror.is_keyless());
        let question = add::question(&mirror);
        assert!(
            question.contains("Unverified") && question.contains("never a plugin"),
            "{question}"
        );
        assert!(
            add::proposed(&config, "https://far.example.com/i.json", "", Some("Far!")).is_err()
        );
    }
}
