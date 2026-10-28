//! What every thread is built from: the database, the model, the config and the paths.
//! The runtime builds chat threads with it, and `ask_agent` builds subagent threads.

use std::{path::Path, sync::Arc};

use anyhow::Result;

use super::{
    subagent::{AskAgent, MAX_AGENT_DEPTH},
    thread_runner::ThreadSetup,
};
use crate::{
    graph::Node,
    live_config::LiveConfig,
    paths::Paths,
    providers::{Model, ModelProvider},
    state::State,
    tools::{ReadHistory, ToolRegistry, ToolScope},
};

#[derive(Clone)]
pub struct Launcher {
    pub state: Arc<State>,
    pub provider: Arc<dyn ModelProvider>,
    pub model: Model,
    pub config: Arc<LiveConfig>,
    pub paths: Paths,
}

impl Launcher {
    /// `agent` is the agent the thread works as, `None` for a chat; `depth` counts the
    /// agents above it.
    pub fn setup(
        &self,
        thread_id: &str,
        cwd: &Path,
        workspace: &Path,
        agent: Option<&Node>,
        depth: usize,
    ) -> Result<ThreadSetup> {
        Ok(ThreadSetup {
            state: Arc::clone(&self.state),
            provider: Arc::clone(&self.provider),
            model: self.model.clone(),
            config: Arc::clone(&self.config),
            thread_id: thread_id.to_owned(),
            tools: self.tools(thread_id, agent, depth),
            scope: ToolScope::new(cwd, workspace)?,
            compact_prompt: self.paths.compact_prompt(),
            depth,
        })
    }

    /// The built-in tools, the thread's own history, and other agents while the depth
    /// allows. An agent with a `tools` list keeps only those.
    pub fn tools(&self, thread_id: &str, agent: Option<&Node>, depth: usize) -> ToolRegistry {
        let mut tools = ToolRegistry::builtin(&self.paths.skills_dir());
        let history = ReadHistory::new(Arc::clone(&self.state), thread_id.to_owned());
        tools.add(Box::new(history));
        if depth < MAX_AGENT_DEPTH {
            let asker = agent.map(|agent| agent.name.clone());
            let ask = AskAgent::new(self.clone(), thread_id.to_owned(), asker, depth);
            tools.add(Box::new(ask));
        }
        if let Some(allowed) = agent.and_then(|agent| agent.tools.as_ref()) {
            tools.keep(allowed);
        }
        tools
    }
}
