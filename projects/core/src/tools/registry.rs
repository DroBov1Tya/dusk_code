//! The tools a thread offers, by name: built-in ones and those of MCP servers.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
    time::Instant,
};

use anyhow::Result;
use parking_lot::Mutex;
use serde_json::Value;
use tracing::info;

use super::{
    Shaped,
    schema::Check,
    scope::ToolScope,
    tool::{Progress, Tool, ToolContext, ToolKind},
};
use crate::{
    config::Config,
    platform::Platform,
    providers::{ToolCall, ToolSpec},
};

pub use dusk_protocol::tools::ToolResult;

/// The model reads every result; a runaway one must not flood its context.
pub(super) const MAX_RESULT_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Box<dyn Tool>>,
    /// Compiled once per tool, when it is added.
    checks: BTreeMap<String, Check>,
    /// A chat's tools follow the switches of the chat's mode; an agent's follow MAIN's.
    pub chat: bool,
    /// Set only from trusted launcher depth, never from model/config identity.
    root: bool,
    pub(crate) jobs: Option<Arc<crate::runtime::jobs::JobManager>>,
    pub(crate) approvals: Option<Arc<super::Approvals>>,
    /// What the machine offers; set by the launcher from what it found at start.
    pub(crate) platform: Platform,
    /// The record, where each thread's own approval mode lives.
    pub(crate) state: Option<Arc<crate::state::State>>,
    /// The thread these tools run for; its own mode applies.
    pub(crate) thread_id: String,
    /// With `limits.stable_prefix`, each description as this thread first showed it, so
    /// the start of its requests stays byte-identical and the provider's cache holds.
    shown: Mutex<BTreeMap<String, String>>,
}

impl ToolRegistry {
    /// The built-in tools; `skills` is the skill directory `load_skill` reads.
    pub fn builtin(skills: &Path) -> Self {
        let mut registry = Self::default();
        for tool in super::builtin::all(skills) {
            registry.add(tool);
        }
        registry
    }

    pub(crate) async fn job_wake_state(&self, thread_id: &str) -> Result<(bool, bool)> {
        match &self.jobs {
            Some(jobs) => jobs.wake_state(thread_id).await,
            None => Ok((false, false)),
        }
    }

    pub(crate) async fn has_running_jobs(&self, thread_id: &str) -> bool {
        match &self.jobs {
            Some(jobs) => jobs.has_running(thread_id).await,
            None => false,
        }
    }

    /// A later tool with the same name replaces the earlier one.
    pub fn add(&mut self, tool: Box<dyn Tool>) {
        // This name belongs to the core tool, including when an MCP server attempts
        // to expose a colliding name in a child thread.
        if matches!(
            tool.name(),
            "display_image" | "display_audio" | dusk_protocol::plugin::SUGGEST_TOOL
        ) && !tool.root_only()
        {
            return;
        }
        let name = tool.name().to_owned();
        self.checks
            .insert(name.clone(), Check::new(&tool.parameters()));
        self.tools.insert(name, tool);
    }

    pub(crate) fn set_depth(&mut self, depth: usize) {
        self.root = depth == 0;
    }

    /// Drops every tool not named in `names`.
    pub fn keep(&mut self, names: &[String]) {
        self.tools
            .retain(|name, _| names.iter().any(|kept| kept == name));
        let tools = &self.tools;
        self.checks.retain(|name, _| tools.contains_key(name));
    }

    pub fn kind_of(&self, name: &str) -> Option<ToolKind> {
        self.tools.get(name).map(|tool| tool.kind())
    }

    /// Whether the call may simply run again after its result was lost; an unknown tool
    /// is never repeated.
    pub fn repeatable(&self, name: &str, arguments: &Value) -> bool {
        self.tools
            .get(name)
            .is_some_and(|tool| tool.repeatable(arguments))
    }

    /// The result of a call as the model is to be sent it, when the tool shapes it.
    pub fn shape(&self, name: &str, arguments: &Value, result: &ToolResult) -> Option<Shaped> {
        self.tools.get(name)?.shape(arguments, result)
    }

    /// The line naming a narrower command to run next, when the tool has one.
    pub fn rerun(&self, name: &str, arguments: &Value, result: &ToolResult) -> Option<String> {
        self.tools.get(name)?.rerun(arguments, result)
    }

    /// Whether the call only reads, so it may run with the read-only calls next to it;
    /// an unknown tool never does.
    pub fn read_only(&self, name: &str, arguments: &Value) -> bool {
        self.tools
            .get(name)
            .is_some_and(|tool| tool.read_only(arguments))
    }

    /// The skill a successful `load_skill` call named; `None` for other tools or when
    /// the arguments do not name one.
    pub fn loaded_skill(&self, name: &str, arguments: &serde_json::Value) -> Option<String> {
        if name != "load_skill" {
            return None;
        }
        Some(arguments.get("name")?.as_str()?.to_owned())
    }

    #[cfg(test)]
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    /// Every tool with its kind, by kind and then by name.
    pub fn kinds(&self) -> Vec<(String, ToolKind)> {
        let mut kinds: Vec<_> = self
            .tools
            .iter()
            .filter(|(_, tool)| tool.switchable() && self.platform.provides(tool.needs()))
            .map(|(name, tool)| (name.clone(), tool.kind()))
            .collect();
        kinds.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        kinds
    }

    /// The calls `name` runs inside it, for hooks to judge one by one; empty for a tool
    /// that runs none or arguments that do not parse, which the call itself refuses.
    pub fn calls_within(&self, name: &str, arguments: &Value) -> Vec<(String, Value)> {
        self.tools
            .get(name)
            .and_then(|tool| tool.calls_within(arguments).ok())
            .unwrap_or_default()
    }

    /// A call inside another is refused as it would be alone: a tool switched off in the
    /// mode does not run because a batch names it.
    fn check_within(
        &self,
        tool: &dyn Tool,
        arguments: &Value,
        config: &Config,
    ) -> std::result::Result<(), String> {
        let within = tool
            .calls_within(arguments)
            .map_err(|error| format!("{error:#}"))?;
        for (index, (name, _)) in within.iter().enumerate() {
            let on = self
                .tools
                .get(name.as_str())
                .is_some_and(|inner| self.enabled(inner.as_ref(), config));
            if !on {
                return Err(format!(
                    "step {} ({name}) is switched off, so no step ran",
                    index + 1
                ));
            }
        }
        Ok(())
    }

    fn enabled(&self, tool: &dyn Tool, config: &Config) -> bool {
        if tool.root_only() && !self.root || !self.platform.provides(tool.needs()) {
            return false;
        }
        if !tool.switchable() {
            return true;
        }
        match self.chat {
            true => config.mode_tool_enabled(config.mode(), tool.name(), tool.kind()),
            false => {
                config.mode_tool_enabled(crate::catalog::modes::MAIN, tool.name(), tool.kind())
            }
        }
    }

    /// What each tool tells the model it does, switched off or not, by name.
    pub fn descriptions(
        &self,
        scope: &Arc<ToolScope>,
        config: &Arc<Config>,
    ) -> HashMap<String, String> {
        let context = ToolContext {
            scope: Arc::clone(scope),
            config: Arc::clone(config),
            progress: None,
            approvals: None,
            state: self.state.clone(),
            thread_id: self.thread_id.clone(),
        };
        self.tools
            .iter()
            .map(|(name, tool)| (name.clone(), tool.description(&context)))
            .collect()
    }

    /// The tools the config has on, as the model sees them.
    pub fn specs(&self, scope: &Arc<ToolScope>, config: &Arc<Config>) -> Vec<ToolSpec> {
        let context = ToolContext {
            scope: Arc::clone(scope),
            config: Arc::clone(config),
            progress: None,
            approvals: None,
            state: self.state.clone(),
            thread_id: self.thread_id.clone(),
        };
        let stable = config.limits().stable_prefix;
        self.tools
            .values()
            .filter(|tool| self.enabled(tool.as_ref(), config) && tool.available(&context))
            .map(|tool| ToolSpec {
                name: tool.name().to_owned(),
                description: self.description(tool.as_ref(), &context, stable),
                parameters: tool.offered_parameters(&context),
            })
            .collect()
    }

    /// What the model reads of `tool`: as it is now, or with `stable` as first shown.
    fn description(&self, tool: &dyn Tool, context: &ToolContext, stable: bool) -> String {
        if !stable {
            return tool.description(context);
        }
        if let Some(shown) = self.shown.lock().get(tool.name()) {
            return shown.clone();
        }
        // Rendered outside the lock: a description may read files.
        let description = tool.description(context);
        let mut shown = self.shown.lock();
        shown
            .entry(tool.name().to_owned())
            .or_insert(description)
            .clone()
    }

    /// Descriptions are rendered anew from the next request. A compaction calls it: the
    /// calls that showed the model what changed are summarized away, and its new context
    /// is read cold anyway.
    pub fn refresh_descriptions(&self) {
        self.shown.lock().clear();
    }

    /// Never fails: an unknown, switched-off or failed call becomes an error result for
    /// the model. A model may call a tool it was not offered, so the switch is checked here.
    #[tracing::instrument(
        name = "tool_call",
        skip_all,
        fields(tool = %call.name, failed = tracing::field::Empty)
    )]
    pub async fn call(
        &self,
        call: &ToolCall,
        scope: &Arc<ToolScope>,
        config: &Arc<Config>,
        progress: Progress,
    ) -> ToolResult {
        let Some(tool) = self.tools.get(call.name.as_str()) else {
            return error_result(format!("unknown tool {}", call.name));
        };
        if !self.enabled(tool.as_ref(), config) {
            return error_result(format!("tool {} is switched off", call.name));
        }
        let arguments = match &call.arguments {
            Value::Object(_) => call.arguments.clone(),
            Value::String(raw) => {
                let error = serde_json::from_str::<Value>(raw).err();
                let reason = error.map_or("not an object".into(), |error| error.to_string());
                return error_result(format!("arguments are not valid JSON ({reason})"));
            }
            _ => return error_result("arguments must be a JSON object".into()),
        };
        if let Some(check) = self.checks.get(call.name.as_str())
            && let Some(problems) = check.problems(&call.name, &tool.parameters(), &arguments)
        {
            info!("tool {} called with invalid arguments", call.name);
            return error_result(problems);
        }
        if let Err(problem) = self.check_within(tool.as_ref(), &arguments, config) {
            return error_result(problem);
        }
        let context = ToolContext {
            scope: Arc::clone(scope),
            config: Arc::clone(config),
            progress: Some(progress),
            approvals: self.approvals.clone(),
            state: self.state.clone(),
            thread_id: self.thread_id.clone(),
        };
        let started = Instant::now();
        if !tool.available(&context) {
            return error_result(format!("tool {} is unavailable", call.name));
        }
        let result = tool.call_result(arguments, context).await;
        let took = started.elapsed().as_millis();
        let failed = result.as_ref().map_or(true, |result| result.is_error);
        tracing::Span::current().record("failed", failed);
        match result {
            Ok(mut result) => {
                info!("tool {} done in {took} ms", call.name);
                result.content = bounded(result.content);
                result
            }
            Err(error) => {
                // Name and time only: arguments, output and errors may quote files.
                info!("tool {} failed in {took} ms", call.name);
                error_result(format!("{error:#}"))
            }
        }
    }
}

fn error_result(message: String) -> ToolResult {
    ToolResult {
        content: bounded(format!("error: {message}")),
        is_error: true,
        exit_code: None,
        artifacts: Vec::new(),
    }
}

fn bounded(mut content: String) -> String {
    if content.trim().is_empty() {
        return "(no output)".into();
    }
    if content.len() > MAX_RESULT_BYTES {
        content.truncate(content.floor_char_boundary(MAX_RESULT_BYTES));
        content.push_str("\n[output truncated]");
    }
    content
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        state::artifacts::ArtifactStore,
        testing::{temp_dir, temp_state},
        tools::{
            AgentState, AskUser, DisplayImage, GoalTool, PlanEdit, ReadHistory, Remember, Show,
            SuggestNext, Tasks,
        },
    };

    struct NeedsScreen;

    impl Tool for NeedsScreen {
        fn name(&self) -> &str {
            "screen"
        }
        fn needs(&self) -> crate::platform::Needs {
            crate::platform::Needs::Gui
        }
        fn description(&self, _: &ToolContext) -> String {
            String::new()
        }
        fn parameters(&self) -> Value {
            serde_json::json!({"type": "object"})
        }
        fn call<'a>(&'a self, _: Value, _: ToolContext) -> crate::BoxFuture<'a, String> {
            Box::pin(async { Ok(String::new()) })
        }
    }

    /// Without a screen, a tool that needs one is neither offered nor switchable.
    #[test]
    fn a_tool_needing_a_screen_exists_only_with_one() {
        let dir = temp_dir("registry_needs");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ToolScope::new(&dir, &dir).unwrap();
        let context = crate::testing::tool_context(&scope);
        let mut registry = ToolRegistry::default();
        registry.add(Box::new(NeedsScreen));
        let offered = |registry: &ToolRegistry| {
            registry
                .specs(&context.scope, &context.config)
                .iter()
                .any(|spec| spec.name == "screen")
        };
        assert!(!offered(&registry));
        assert!(registry.kinds().is_empty());
        registry.platform.gui = true;
        assert!(offered(&registry));
        assert_eq!(registry.kinds().len(), 1);
    }

    /// A batch cannot run a tool the mode switched off: refused before anything is asked.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[tokio::test]
    async fn a_batch_step_switched_off_refuses_the_batch() {
        use crate::tools::{ComputerActions, PressKeys, Screen, TypeText};
        let dir = temp_dir("registry_batch_switch");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let mut config = (*crate::testing::tool_context(&scope).config).clone();
        for (name, on) in [
            ("computer_actions", true),
            ("press_keys", true),
            ("type_text", false),
        ] {
            config.set_tool_enabled(name, on);
        }
        let config = Arc::new(config);
        let screen = Arc::new(Screen::new(ArtifactStore::new(dir.clone())));
        let mut registry = ToolRegistry {
            root: true,
            ..ToolRegistry::default()
        };
        registry.platform.gui = true;
        registry.add(Box::new(ComputerActions(Arc::clone(&screen))));
        registry.add(Box::new(PressKeys(Arc::clone(&screen))));
        registry.add(Box::new(TypeText(screen)));
        let call = ToolCall {
            id: "c".into(),
            name: "computer_actions".into(),
            arguments: serde_json::json!({"steps": [
                {"tool": "press_keys", "arguments": {"keys": "enter"}},
                {"tool": "type_text", "arguments": {"text": "hi"}},
            ]}),
        };
        let (events, mut asked) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let result = registry.call(&call, &scope, &config, progress).await;
        assert!(result.is_error);
        assert!(
            result
                .content
                .contains("step 2 (type_text) is switched off"),
            "{}",
            result.content
        );
        assert!(asked.try_recv().is_err(), "nothing may be asked");
    }

    /// The real pointer is never offered until the user switches it on, in any mode.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn physical_input_is_offered_only_when_switched_on() {
        use crate::tools::{PhysicalInput, Screen};
        let dir = temp_dir("registry_physical");
        std::fs::create_dir_all(&dir).unwrap();
        let scope = Arc::new(ToolScope::new(&dir, &dir).unwrap());
        let screen = Arc::new(Screen::new(ArtifactStore::new(dir.clone())));
        let mut registry = ToolRegistry {
            root: true,
            ..ToolRegistry::default()
        };
        registry.platform.gui = true;
        registry.add(Box::new(PhysicalInput(screen)));
        let mut config = (*crate::testing::tool_context(&scope).config).clone();
        let offered = |config: &crate::config::Config| {
            registry
                .specs(&scope, &Arc::new(config.clone()))
                .iter()
                .any(|spec| spec.name == "physical_input")
        };
        assert!(!offered(&config), "off by default");
        config.set_mode_tool_enabled("plan", "physical_input", true);
        assert!(offered(&config), "the switch is one for every mode");
        config.set_tool_enabled("physical_input", false);
        assert!(!offered(&config));
    }

    /// Providers reject a schema that is not one plain object at the top.
    #[tokio::test]
    async fn every_schema_is_one_object_providers_accept() {
        let state = Arc::new(temp_state("registry_schemas").await);
        let dir = temp_dir("registry_schemas");
        let mut tools: Vec<Box<dyn Tool>> = super::super::builtin::all(&dir);
        tools.push(Box::new(Tasks::new(Arc::clone(&state), "t".into())));
        tools.push(Box::new(GoalTool::new(Arc::clone(&state), "t".into())));
        tools.push(Box::new(PlanEdit::new(Arc::clone(&state), "t".into())));
        tools.push(Box::new(ReadHistory::new(Arc::clone(&state), "t".into())));
        tools.push(Box::new(Remember::user(dir.clone())));
        tools.push(Box::new(DisplayImage::new(ArtifactStore::new(dir.clone()))));
        tools.push(Box::new(AskUser));
        tools.push(Box::new(Show));
        let paths = crate::init::paths::Paths::new(dir.join("config"), dir.join("work"));
        tools.push(Box::new(crate::tools::SearchWorkshop::new(paths.clone())));
        tools.push(Box::new(crate::tools::SuggestFromWorkshop::new(paths)));
        tools.push(Box::new(SuggestNext));
        tools.push(Box::new(AgentState::new(Arc::clone(&state), "t".into())));
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use crate::tools::{
                Click, ComputerActions, PhysicalInput, PressKeys, Screen, Screenshot, TypeText,
            };
            let screen = Arc::new(Screen::new(ArtifactStore::new(dir.clone())));
            tools.push(Box::new(Screenshot(Arc::clone(&screen))));
            tools.push(Box::new(Click(Arc::clone(&screen))));
            tools.push(Box::new(PhysicalInput(Arc::clone(&screen))));
            tools.push(Box::new(TypeText(Arc::clone(&screen))));
            tools.push(Box::new(PressKeys(Arc::clone(&screen))));
            tools.push(Box::new(ComputerActions(Arc::clone(&screen))));
            #[cfg(target_os = "macos")]
            tools.push(Box::new(crate::tools::WindowSession(screen)));
        }
        for tool in tools {
            let schema = tool.parameters();
            let name = tool.name();
            assert_eq!(schema["type"], "object", "{name}");
            for key in ["oneOf", "anyOf", "allOf", "$ref", "$defs", "$schema"] {
                assert!(schema.get(key).is_none(), "{name} has {key} at the top");
            }
            assert!(
                jsonschema::Validator::new(&schema).is_ok(),
                "{name} schema does not compile"
            );
        }
    }
}
