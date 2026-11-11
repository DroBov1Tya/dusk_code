//! Agents: switching them, stopping them, editing their files.

use crate::runtime::Runtime;
use crate::runtime::agent::Input;
use crate::{
    catalog::agents::AgentTree,
    catalog::graph::{AGENT_FILE, NodeText, read_node_text, save_node_text},
};
use anyhow::{Context, Result, ensure};
use tracing::info;

impl Runtime {
    /// Reads the agents folder now instead of at the next minute.
    pub async fn read_agents(&self) -> Result<()> {
        self.launcher.agents.scan().await
    }

    /// The chat and the agents it can reach, as deep as they may ask each other.
    pub fn agent_tree(&self) -> AgentTree {
        self.launcher.agents.tree(&self.launcher.config.current())
    }

    pub fn set_agent_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_agent_enabled(name, enabled))?;
        Ok(())
    }

    /// Stops one agent and every agent it asked; agents waiting for a free slot drop
    /// their waiting messages. The chat and other agents go on.
    pub async fn stop_agent(&self, thread_id: &str) -> Result<()> {
        let state = &self.launcher.state;
        let thread = state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        ensure!(
            thread.parent_thread_id.is_some(),
            "thread {thread_id} is not an agent's"
        );
        let root = state.root_thread(thread_id).await?;
        let mut threads = state.descendant_threads(thread_id).await?;
        threads.insert(0, thread_id.to_owned());
        for stopped in threads {
            match self.launcher.mailboxes.get(&stopped).await {
                Some(mailbox) => {
                    mailbox.send(Input::Cancel);
                }
                None => state.drop_pending(&stopped).await?,
            }
        }
        // A running agent reports its stop from its turn; one still waiting for a slot
        // has no turn, so the asker is told here.
        if self.launcher.mailboxes.get(thread_id).await.is_none()
            && let Some(parent) = &thread.parent_thread_id
        {
            let stopped = "was stopped before it started";
            self.launcher
                .tell_stopped(parent, thread_id, stopped)
                .await?;
        }
        // A queued agent has no runner/report stream to release a waiting goal.
        // Its requests are durably dropped before the non-preemptive wake.
        if let Some(mailbox) = self.launcher.mailboxes.get(&root).await {
            mailbox.send(Input::DependencyFinished);
        }
        info!("agent thread {thread_id} stopped by the user");
        Ok(())
    }

    /// An agent's AGENT.md as it is on disk; reading it again throws edits away.
    pub fn agent(&self, name: &str) -> Result<NodeText> {
        read_node_text(&self.launcher.paths.agents_dir(), AGENT_FILE, name)
    }

    /// Saves an agent's AGENT.md once the text reads as an agent, and reads the agents
    /// again so the new version applies now.
    pub async fn save_agent(&self, name: &str, text: &str) -> Result<NodeText> {
        let saved = save_node_text(&self.launcher.paths.agents_dir(), AGENT_FILE, name, text)?;
        self.read_agents().await?;
        Ok(saved)
    }
}
