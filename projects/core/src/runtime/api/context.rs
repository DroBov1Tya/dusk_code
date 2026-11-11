//! What the next request of a thread carries.

use crate::runtime::Runtime;
use crate::{
    catalog::modes::Modes,
    catalog::skills::{self},
    message::commands::{
        ContextDetail, ContextReport, McpServerDetail, MemoryFile, context_report,
    },
    tools::{McpTool, ToolScope, project_folder},
};
use anyhow::Result;
use std::{path::Path, sync::Arc};

impl Runtime {
    /// What the next request of a chat thread carries; before a thread, the mode's prompt and the
    /// tools a chat gets.
    pub async fn context(&self, thread_id: Option<&str>) -> Result<ContextReport> {
        let launcher = &self.launcher;
        let config = launcher.config.current();
        let cwd = std::env::current_dir()?;
        let scope = Arc::new(ToolScope::new(&cwd, &cwd)?);
        let specs = launcher
            .tools(thread_id.unwrap_or_default(), None, 0)
            .specs(&scope, &config);
        let mode = config.mode().to_owned();
        let prompt = Modes::new(&launcher.paths.modes_dir()).prompt(&mode)?;
        let limit = self
            .model()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(&config)
            });
        let detail = self.context_detail(&cwd);
        context_report(
            &launcher.state,
            thread_id,
            &mode,
            prompt,
            &specs,
            limit,
            &detail,
        )
        .await
    }

    /// What the context report breaks down per item: the skills on now, the servers
    /// ready for it, and the memory files that ride in every request.
    pub(super) fn context_detail(&self, cwd: &Path) -> ContextDetail {
        let launcher = &self.launcher;
        let config = launcher.config.current();
        let skills = skills::list(&launcher.paths.skills_dir(), &config)
            .into_iter()
            .filter(|skill| skill.enabled && skill.problem.is_none())
            .map(|skill| (skill.name, skill.description))
            .collect();
        let mcp = launcher
            .mcp
            .ready()
            .iter()
            .filter_map(|server| {
                let detail = McpServerDetail {
                    server: server.name.clone(),
                    tools: server
                        .tools
                        .iter()
                        .filter_map(|tool| McpTool::model_name(&server.name, &tool.name))
                        .collect(),
                    instructions: server.client.instructions(),
                };
                (!detail.tools.is_empty() || detail.instructions.is_some()).then_some(detail)
            })
            .collect();
        let mut memory = Vec::new();
        if config.memory().enabled {
            memory.push(MemoryFile {
                label: "remember".into(),
                path: launcher.paths.rules_root().join("MEMORY.md"),
                max_chars: config.memory().max_chars,
            });
        }
        if config.work_history().enabled {
            memory.push(MemoryFile {
                label: "remember_project".into(),
                path: launcher
                    .paths
                    .workspace_root()
                    .join("projects")
                    .join(project_folder(cwd))
                    .join("MEMORY.md"),
                max_chars: config.work_history().max_chars,
            });
        }
        ContextDetail {
            skills,
            mcp,
            memory,
        }
    }
}
