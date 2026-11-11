//! The settings view: its rows, selection and the actions a row gives.

use super::editor::{FileEditor, flatten};
use super::types::{
    ACCESS_POLICIES, AgentRow, EditTarget, FileButton, McpRow, SettingCategory, SettingsHit,
    SwitchKind,
};
use crate::app::{Action, Input};
use dusk_protocol::{
    catalog::{AgentTree, Problem, SkillInfo},
    client::Command,
    config::{
        ApprovalPolicy, ContextPolicy, ContextPreset, McpInfo, McpState, Memory, Settings,
        ToolInfo, WorkHistory,
    },
    model::Effort,
    release::Channel,
    system::SystemReport,
};
use std::{collections::BTreeSet, time::Instant};

pub struct SettingsView {
    pub selected_model: Option<String>,
    pub context_policy: Option<ContextPolicy>,
    pub effective_limit: u64,
    pub effort: Option<Effort>,
    pub manual_context: Option<Input>,
    /// The branch prefix saved, and the box it is edited in while one is open.
    pub branch_prefix: String,
    pub branch_input: Option<Input>,
    pub model_error: Option<String>,
    pub max_agents_at_work: usize,
    pub max_goal_rounds: u32,
    /// 0 never starts the idle screen.
    pub idle_screen_minutes: u32,
    pub memory: Memory,
    pub work_history: WorkHistory,
    pub update_channel: Channel,
    pub approvals: ApprovalPolicy,
    pub sandbox: bool,
    pub logging: bool,
    /// The mode whose tools are shown.
    pub mode: String,
    pub tools: Vec<ToolInfo>,
    pub skills: Vec<SkillInfo>,
    pub mcp: Vec<McpInfo>,
    /// The MCP servers whose tools are listed under them.
    pub mcp_open: BTreeSet<String>,
    /// The chat first, then every agent under it.
    pub agents: Vec<AgentRow>,
    pub problems: Vec<Problem>,
    /// Links to agents that are not there; shown as notes, since the rest works.
    pub skipped: Vec<Problem>,
    /// `None` is the category menu.
    pub category: Option<SettingCategory>,
    pub selected: usize,
    /// The skill or MCP server open beside the list.
    pub editor: Option<FileEditor>,
    /// The system tab's numbers and when they were read; refreshed while it is open.
    pub system: Option<(SystemReport, Instant)>,
    /// What can be clicked on the last drawn frame.
    pub hits: Vec<SettingsHit>,
    /// The first list row shown.
    pub top: usize,
}

/// The agent limit takes the first row of the agents list.
pub(super) const LIMIT_ROWS: usize = 2;

/// The memory's size moves in steps this large, within these bounds.
pub(super) const MEMORY_STEP: usize = 500;

pub(super) const MEMORY_CHARS: std::ops::RangeInclusive<usize> = 500..=20_000;

pub(super) const PAGE: usize = 20;

pub(super) const WHEEL: usize = 3;

impl SettingsView {
    pub fn new(settings: Settings) -> Self {
        let mut agents = Vec::new();
        flatten(&settings.agents.root, 0, &[], true, true, &mut agents);
        let AgentTree {
            problems, skipped, ..
        } = settings.agents;
        Self {
            system: None,
            selected_model: None,
            context_policy: None,
            effective_limit: 0,
            effort: None,
            manual_context: None,
            branch_prefix: settings.version_control.branch_prefix,
            branch_input: None,
            model_error: None,
            max_agents_at_work: settings.max_agents_at_work,
            max_goal_rounds: settings.max_goal_rounds,
            idle_screen_minutes: settings.idle_screen_minutes,
            mode: settings.mode,
            memory: settings.memory,
            work_history: settings.work_history,
            update_channel: settings.update_channel,
            approvals: settings.approvals,
            sandbox: settings.sandbox,
            logging: settings.logging,
            tools: settings.tools,
            skills: settings.skills,
            mcp: settings.mcp,
            mcp_open: BTreeSet::new(),
            agents,
            problems,
            skipped,
            category: None,
            selected: 0,
            editor: None,
            hits: Vec::new(),
            top: 0,
        }
    }

    /// Fresh settings in place of these, keeping where the user is.
    pub fn refresh(&mut self, settings: Settings) {
        let mut next = Self::new(settings);
        next.selected_model = self.selected_model.clone();
        next.context_policy = self.context_policy;
        next.effective_limit = self.effective_limit;
        next.effort = self.effort;
        next.manual_context = self.manual_context.take();
        next.branch_input = self.branch_input.take();
        next.model_error = self.model_error.take();
        next.category = self.category;
        next.selected = self.selected.min(next.len().saturating_sub(1));
        next.editor = self.editor.take();
        next.system = self.system.take();
        next.mcp_open = std::mem::take(&mut self.mcp_open);
        next.top = self.top;
        *self = next;
    }

    pub fn len(&self) -> usize {
        match self.category {
            None => SettingCategory::ALL.len(),
            Some(SettingCategory::Models) => 7,
            Some(SettingCategory::Agents) => LIMIT_ROWS + self.agents.len(),
            Some(SettingCategory::Tools) => self.tools.len(),
            Some(SettingCategory::Personalize) => 4,
            Some(SettingCategory::Limits) => 2,
            Some(SettingCategory::Appearance) => 1,
            Some(SettingCategory::Updates) => 1,
            Some(SettingCategory::Access) => ACCESS_POLICIES.len() + 1,
            Some(SettingCategory::VersionControl) => 1,
            Some(SettingCategory::System) => 1,
            Some(SettingCategory::Skills) => self.skills.len(),
            Some(SettingCategory::Mcp) => self.mcp_rows().len(),
        }
    }

    /// The MCP list as shown: each server, then its tools when they are open.
    pub fn mcp_rows(&self) -> Vec<McpRow<'_>> {
        let mut rows = Vec::new();
        for server in &self.mcp {
            rows.push(McpRow::Server(server));
            if self.mcp_open.contains(&server.name) {
                rows.extend(server.tools.iter().map(|tool| McpRow::Tool(server, tool)));
            }
        }
        rows
    }

    pub(super) fn mcp_server(&self, row: usize) -> Option<&McpInfo> {
        match self.mcp_rows().get(row)? {
            McpRow::Server(server) | McpRow::Tool(server, _) => Some(server),
        }
    }

    /// Lists the tools of the server on `row`, or hides them and selects the server.
    pub(super) fn toggle_mcp_tools(&mut self, row: usize) {
        let Some(name) = self.mcp_server(row).map(|server| server.name.clone()) else {
            return;
        };
        if !self.mcp_open.remove(&name) {
            self.mcp_open.insert(name);
            return;
        }
        self.selected = self
            .mcp_rows()
            .iter()
            .position(|row| matches!(row, McpRow::Server(server) if server.name == name))
            .unwrap_or(0);
    }

    pub(super) fn model_action(&self, row: usize) -> Option<Action> {
        self.selected_model.as_ref()?;
        match row {
            0 => Some(Action::SetContextPolicy(ContextPolicy::Auto)),
            1..=4 => Some(Action::SetContextPolicy(ContextPolicy::Fixed(
                ContextPreset::ALL[row - 1],
            ))),
            6 => Some(Action::Command(Command::Effort { level: None })),
            _ => None,
        }
    }

    pub fn edit_branch_prefix(&mut self) {
        let mut input = Input::default();
        input.set(self.branch_prefix.clone());
        input.select_all();
        self.branch_input = Some(input);
    }

    pub(super) fn edit_manual_context(&mut self) {
        if self.selected_model.is_none() {
            self.model_error = Some("choose a model with /model first".into());
            return;
        }
        let tokens = match self.context_policy {
            Some(ContextPolicy::Manual(tokens)) => tokens,
            _ => self.effective_limit,
        };
        let mut input = Input::default();
        input.set(tokens.to_string());
        input.select_all();
        self.manual_context = Some(input);
        self.model_error = None;
    }

    /// The agent on a row of the agents list; the limit row and the chat have none.
    pub(super) fn agent(&self, row: usize) -> Option<&AgentRow> {
        let agent = self.agents.get(row.checked_sub(LIMIT_ROWS)?)?;
        (agent.depth > 0).then_some(agent)
    }

    /// The agent on a row that has an AGENT.md to edit.
    pub(super) fn agent_file(&self, row: usize) -> Option<String> {
        let agent = self.agent(row)?;
        agent.version.map(|_| agent.name.clone())
    }

    /// What moving the limit on the selected row asks of the core.
    pub(super) fn limit_action(&self, more: bool) -> Action {
        let counted = |value: u64| match more {
            true => value.saturating_add(1),
            false => value.saturating_sub(1).max(1),
        };
        match self.selected {
            0 => Action::SetAgentLimit(counted(self.max_agents_at_work as u64) as usize),
            _ => Action::SetGoalRounds(counted(u64::from(self.max_goal_rounds)) as u32),
        }
    }

    pub(super) fn memory_size_action(&self, row: usize, more: bool) -> Action {
        let current = match row {
            1 => self.memory.max_chars,
            3 => self.work_history.max_chars,
            _ => return Action::None,
        };
        let chars = current - current % MEMORY_STEP;
        let chars = if more {
            chars.saturating_add(MEMORY_STEP)
        } else if chars == current {
            chars.saturating_sub(MEMORY_STEP)
        } else {
            chars
        };
        let chars = chars.clamp(*MEMORY_CHARS.start(), *MEMORY_CHARS.end());
        if chars == current {
            return Action::None;
        }
        match row {
            1 => Action::SetMemoryChars(chars),
            _ => Action::SetWorkHistoryChars(chars),
        }
    }

    /// What switching `row` asks of the core.
    pub(super) fn switch(&self, row: usize) -> Option<Action> {
        let (kind, name, on) = match self.category? {
            SettingCategory::Models => return self.model_action(row),
            SettingCategory::Agents => {
                let agent = self.agent(row)?;
                (SwitchKind::Agent, agent.name.clone(), agent.enabled)
            }
            SettingCategory::Tools => {
                let tool = self.tools.get(row)?;
                (SwitchKind::Tool, tool.name.clone(), tool.enabled)
            }
            SettingCategory::Skills => {
                let skill = self.skills.get(row)?;
                (SwitchKind::Skill, skill.name.clone(), skill.enabled)
            }
            SettingCategory::Mcp => match *self.mcp_rows().get(row)? {
                McpRow::Server(server) => {
                    let on = server.state != McpState::Off;
                    (SwitchKind::Mcp, server.name.clone(), on)
                }
                McpRow::Tool(_, tool) => (SwitchKind::Tool, tool.name.clone(), tool.enabled),
            },
            SettingCategory::Personalize if row == 2 => (
                SwitchKind::WorkHistory,
                "project memory".into(),
                self.work_history.enabled,
            ),
            SettingCategory::Personalize if row != 0 => return None,
            SettingCategory::Limits
            | SettingCategory::Appearance
            | SettingCategory::VersionControl => return None,
            SettingCategory::Personalize => {
                (SwitchKind::Memory, "memory".into(), self.memory.enabled)
            }
            SettingCategory::Updates => {
                let dev = self.update_channel == Channel::Dev;
                (SwitchKind::DevChannel, "dev".into(), dev)
            }
            SettingCategory::Access if row < ACCESS_POLICIES.len() => {
                let policy = ACCESS_POLICIES[row];
                (
                    SwitchKind::Approvals(policy),
                    policy.name().into(),
                    self.approvals == policy,
                )
            }
            SettingCategory::Access => (SwitchKind::Logging, "logs".into(), self.logging),
            SettingCategory::System => return None,
        };
        Some(Action::Switch {
            kind,
            name,
            on: !on,
        })
    }
}

impl SettingsView {
    /// A click opens a category or a file, or selects a row; a second click on a selected
    /// row switches it.
    pub fn click_row(&mut self, row: usize) -> Action {
        match self.category {
            None => {
                self.category = SettingCategory::ALL.get(row).copied();
                self.selected = 0;
                self.top = 0;
            }
            Some(SettingCategory::VersionControl) => {
                self.selected = row;
                if self.branch_input.is_none() {
                    self.edit_branch_prefix();
                }
            }
            Some(SettingCategory::Models) => {
                self.selected = row;
                if row == 5 {
                    if self.manual_context.is_none() {
                        self.edit_manual_context();
                    }
                } else {
                    self.manual_context = None;
                    return self.model_action(row).unwrap_or(Action::None);
                }
            }
            Some(category) if self.opens_file(category, row) => {
                self.selected = row;
                let unsaved = self.editor.as_ref().is_some_and(FileEditor::changed);
                if let Some(editor) = self.editor.as_mut().filter(|_| unsaved) {
                    editor.error = Some("save or revert this one first".into());
                    return Action::None;
                }
                return match category {
                    SettingCategory::Agents => self.agent_file(row).map(Action::OpenAgentFile),
                    SettingCategory::Skills => self
                        .skills
                        .get(row)
                        .map(|skill| Action::OpenSkill(skill.name.clone())),
                    _ => self
                        .mcp_server(row)
                        .map(|server| Action::OpenMcp(Some(server.name.clone()))),
                }
                .unwrap_or(Action::None);
            }
            Some(_) if self.selected == row => return self.switch(row).unwrap_or(Action::None),
            Some(_) => self.selected = row,
        }
        Action::None
    }

    /// Whether a click on `row` opens a file beside the list rather than selecting it.
    pub(super) fn opens_file(&self, category: SettingCategory, row: usize) -> bool {
        match category {
            SettingCategory::Agents => self.agent_file(row).is_some(),
            SettingCategory::Skills => true,
            SettingCategory::Mcp => matches!(self.mcp_rows().get(row), Some(McpRow::Server(_))),
            SettingCategory::Models
            | SettingCategory::Tools
            | SettingCategory::Personalize
            | SettingCategory::Limits
            | SettingCategory::Appearance
            | SettingCategory::Updates
            | SettingCategory::Access
            | SettingCategory::VersionControl
            | SettingCategory::System => false,
        }
    }

    pub fn press(&mut self, button: FileButton) -> Action {
        let Some(editor) = &mut self.editor else {
            return Action::None;
        };
        let text = editor.text.text.clone();
        match (button, &editor.target) {
            (FileButton::Save, EditTarget::Skill(name)) => Action::SaveSkill {
                name: name.clone(),
                text,
            },
            (FileButton::Save, EditTarget::Agent(name)) => Action::SaveAgentFile {
                name: name.clone(),
                text,
            },
            (FileButton::Save, EditTarget::Mcp(old)) => Action::SaveMcp {
                old: old.clone(),
                text,
            },
            // Reading the file again drops every change.
            (FileButton::Revert, EditTarget::Skill(name)) => Action::OpenSkill(name.clone()),
            (FileButton::Revert, EditTarget::Agent(name)) => Action::OpenAgentFile(name.clone()),
            (FileButton::Revert, EditTarget::Mcp(name)) => Action::OpenMcp(name.clone()),
            (FileButton::Close, _) if editor.changed() => {
                editor.error = Some("unsaved: save with Ctrl+S or revert with Ctrl+R".into());
                Action::None
            }
            (FileButton::Close, _) => {
                self.editor = None;
                Action::None
            }
        }
    }
}
