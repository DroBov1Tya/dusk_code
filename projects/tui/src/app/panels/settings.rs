//! The settings window, as the core describes it: the agent tree with a switch on each
//! agent, the tools, and the skills and MCP servers, which open beside the list to be
//! read and edited.
//! The window keeps only what is on screen; every change goes to the core, which answers
//! with the settings again.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dusk_core::{
    agents::{AgentNode, AgentTree},
    chat::commands::{Command, Settings, ToolInfo},
    config::Memory,
    graph::NodeText,
    graph::Problem,
    mcp::{McpInfo, McpState},
    skills::SkillInfo,
    update::Channel,
};

use super::{Panel, step};
use crate::app::{Action, App, Input};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingCategory {
    Agents,
    Tools,
    Skills,
    Mcp,
    /// What the agent keeps about the user.
    Personalize,
    Updates,
}

impl SettingCategory {
    pub const ALL: [Self; 6] = [
        Self::Agents,
        Self::Skills,
        Self::Mcp,
        Self::Tools,
        Self::Personalize,
        Self::Updates,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Agents => "agents",
            Self::Tools => "tools",
            Self::Skills => "skills",
            Self::Mcp => "mcp",
            Self::Personalize => "personalize",
            Self::Updates => "updates",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwitchKind {
    Agent,
    Tool,
    Skill,
    Mcp,
    Memory,
    /// On is the dev channel, off the stable one.
    DevChannel,
}

/// One row of the agent tree, with what is needed to draw the traces to it.
pub struct AgentRow {
    pub name: String,
    pub description: String,
    pub version: Option<i64>,
    pub enabled: bool,
    /// The agents it may ask.
    pub links: Vec<String>,
    /// On, and every agent above it too: only then can it be asked.
    pub active: bool,
    pub depth: usize,
    /// For each level above this row: whether the trace goes on below it.
    pub rails: Vec<bool>,
    /// The last of its siblings.
    pub last: bool,
}

pub struct SettingsView {
    pub max_agents_at_work: usize,
    pub memory: Memory,
    pub update_channel: Channel,
    /// The mode whose tools are shown.
    pub mode: String,
    pub tools: Vec<ToolInfo>,
    pub skills: Vec<SkillInfo>,
    pub mcp: Vec<McpInfo>,
    /// The chat first, then every agent under it.
    pub agents: Vec<AgentRow>,
    pub problems: Vec<Problem>,
    /// `None` is the category menu.
    pub category: Option<SettingCategory>,
    pub selected: usize,
    /// The skill or MCP server open beside the list.
    pub editor: Option<FileEditor>,
    /// What can be clicked on the last drawn frame.
    pub hits: Vec<SettingsHit>,
    /// The first list row shown.
    pub top: usize,
}

pub struct FileEditor {
    pub target: EditTarget,
    pub metadata: Vec<(String, String)>,
    pub text: Input,
    original: String,
    pub scroll: usize,
    /// Why the last save was refused.
    pub error: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum EditTarget {
    Skill(String),
    /// An agent's AGENT.md, by the agent's name.
    Agent(String),
    /// `None` for a server not saved yet.
    Mcp(Option<String>),
}

/// What a new MCP server starts as.
const MCP_TEMPLATE: &str = "\
# A local server the core starts. For a remote one, write
#   url: https://host/mcp
#   headers: {Authorization: Bearer ...}
# in place of command, args and env.
name:
  command: npx
  args: [-y, some-mcp-server]
  env: {}
";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileButton {
    Save,
    Revert,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    /// A row of the list, by its index.
    Row(usize),
    /// The switch on a row.
    Switch(usize),
    Button(FileButton),
}

pub struct SettingsHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub target: HitTarget,
}

/// The agent limit takes the first row of the agents list.
const LIMIT_ROWS: usize = 1;
const PAGE: usize = 20;
const WHEEL: usize = 3;

impl SettingsView {
    pub fn new(settings: Settings) -> Self {
        let mut agents = Vec::new();
        flatten(&settings.agents.root, 0, &[], true, true, &mut agents);
        let AgentTree { problems, .. } = settings.agents;
        Self {
            max_agents_at_work: settings.max_agents_at_work,
            mode: settings.mode,
            memory: settings.memory,
            update_channel: settings.update_channel,
            tools: settings.tools,
            skills: settings.skills,
            mcp: settings.mcp,
            agents,
            problems,
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
        next.category = self.category;
        next.selected = self.selected.min(next.len().saturating_sub(1));
        next.editor = self.editor.take();
        next.top = self.top;
        *self = next;
    }

    pub fn len(&self) -> usize {
        match self.category {
            None => SettingCategory::ALL.len(),
            Some(SettingCategory::Agents) => LIMIT_ROWS + self.agents.len(),
            Some(SettingCategory::Tools) => self.tools.len(),
            Some(SettingCategory::Personalize | SettingCategory::Updates) => 1,
            Some(SettingCategory::Skills) => self.skills.len(),
            Some(SettingCategory::Mcp) => self.mcp.len(),
        }
    }

    /// The agent on a row of the agents list; the limit row and the chat have none.
    fn agent(&self, row: usize) -> Option<&AgentRow> {
        let agent = self.agents.get(row.checked_sub(LIMIT_ROWS)?)?;
        (agent.depth > 0).then_some(agent)
    }

    /// The agent on a row that has an AGENT.md to edit.
    fn agent_file(&self, row: usize) -> Option<String> {
        let agent = self.agent(row)?;
        agent.version.map(|_| agent.name.clone())
    }

    /// What switching `row` asks of the core.
    fn switch(&self, row: usize) -> Option<Action> {
        let (kind, name, on) = match self.category? {
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
            SettingCategory::Mcp => {
                let server = self.mcp.get(row)?;
                let on = server.state != McpState::Off;
                (SwitchKind::Mcp, server.name.clone(), on)
            }
            SettingCategory::Personalize => {
                (SwitchKind::Memory, "memory".into(), self.memory.enabled)
            }
            SettingCategory::Updates => {
                let dev = self.update_channel == Channel::Dev;
                (SwitchKind::DevChannel, "dev".into(), dev)
            }
        };
        Some(Action::Switch {
            kind,
            name,
            on: !on,
        })
    }
}

impl FileEditor {
    pub fn skill(skill: NodeText) -> Self {
        Self::new(EditTarget::Skill(skill.name), skill.metadata, skill.text)
    }

    pub fn agent(agent: NodeText) -> Self {
        Self::new(EditTarget::Agent(agent.name), agent.metadata, agent.text)
    }

    /// An MCP server's YAML, or the template for a new one, with how it stands.
    pub fn mcp(server: Option<&McpInfo>, text: Option<String>) -> Self {
        let target = EditTarget::Mcp(server.map(|server| server.name.clone()));
        let text = text.unwrap_or_else(|| MCP_TEMPLATE.to_owned());
        Self::new(target, mcp_fields(server), text)
    }

    /// Shows how the open MCP server stands now; nothing for anything else.
    pub fn update_mcp(&mut self, servers: &[McpInfo]) {
        if let EditTarget::Mcp(Some(name)) = &self.target {
            let server = servers.iter().find(|server| server.name == *name);
            self.metadata = mcp_fields(server);
        }
    }

    fn new(target: EditTarget, metadata: Vec<(String, String)>, text: String) -> Self {
        Self {
            target,
            metadata,
            text: Input {
                text: text.clone(),
                cursor: 0,
                pastes: Vec::new(),
                anchor: None,
            },
            original: text,
            scroll: 0,
            error: None,
        }
    }

    pub fn title(&self) -> &str {
        match &self.target {
            EditTarget::Skill(name) | EditTarget::Agent(name) | EditTarget::Mcp(Some(name)) => name,
            EditTarget::Mcp(None) => "new MCP server",
        }
    }

    pub fn changed(&self) -> bool {
        self.text.text != self.original
    }
}

fn mcp_fields(server: Option<&McpInfo>) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(server) = server {
        let state = match &server.state {
            McpState::Off => "off".to_owned(),
            McpState::Starting => "starting".to_owned(),
            McpState::Ready(tools) => format!("ready, {} tools", tools.len()),
            McpState::Failed(error) => format!("failed: {error}"),
        };
        fields.push(("state".to_owned(), state));
        if let McpState::Ready(tools) = &server.state {
            fields.push(("tools".to_owned(), tools.join(", ")));
        }
    }
    fields.push((
        "help".to_owned(),
        "save starts it again; saving empty text removes it".to_owned(),
    ));
    fields
}

/// The tree as rows, depth first, each knowing the traces of the levels above it.
fn flatten(
    node: &AgentNode,
    depth: usize,
    rails: &[bool],
    last: bool,
    parent_active: bool,
    rows: &mut Vec<AgentRow>,
) {
    let active = parent_active && node.enabled;
    rows.push(AgentRow {
        name: node.name.clone(),
        description: node.description.clone(),
        version: node.version,
        enabled: node.enabled,
        links: node.links.clone(),
        active,
        depth,
        rails: rails.to_vec(),
        last,
    });
    let mut below = rails.to_vec();
    if depth > 0 {
        below.push(!last);
    }
    for (index, child) in node.children.iter().enumerate() {
        let last = index + 1 == node.children.len();
        flatten(child, depth + 1, &below, last, active, rows);
    }
}

impl App {
    pub fn settings_key(&mut self, key: KeyEvent) -> Action {
        let Some(Panel::Settings(view)) = &mut self.panel else {
            return Action::None;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(editor) = &mut view.editor {
            let text = &mut editor.text;
            match key.code {
                KeyCode::Char('s') if ctrl => return press(view, FileButton::Save),
                KeyCode::Char('r') if ctrl => return press(view, FileButton::Revert),
                KeyCode::Esc => return press(view, FileButton::Close),
                KeyCode::Char(c) if !ctrl => text.insert(c.encode_utf8(&mut [0; 4])),
                KeyCode::Enter => text.insert("\n"),
                KeyCode::Tab => text.insert("  "),
                KeyCode::Backspace => text.backspace(),
                KeyCode::Delete => text.delete(),
                KeyCode::Left => text.left(),
                KeyCode::Right => text.right(),
                KeyCode::Home => text.home(),
                KeyCode::End => text.end(),
                KeyCode::Up => {
                    text.vertical(false);
                }
                KeyCode::Down => {
                    text.vertical(true);
                }
                KeyCode::PageUp => move_rows(text, PAGE, false),
                KeyCode::PageDown => move_rows(text, PAGE, true),
                _ => {}
            }
            editor.error = None;
            return Action::None;
        }
        let up = matches!(key.code, KeyCode::Up) || (ctrl && key.code == KeyCode::Char('p'));
        let down = matches!(key.code, KeyCode::Down) || (ctrl && key.code == KeyCode::Char('n'));
        match (view.category, key.code) {
            _ if up || down => {
                let count = view.len();
                step(&mut view.selected, count, down);
            }
            (None, KeyCode::Esc) => self.panel = None,
            (None, KeyCode::Enter | KeyCode::Right) => {
                view.category = SettingCategory::ALL.get(view.selected).copied();
                view.selected = 0;
                view.top = 0;
            }
            (Some(category), KeyCode::Esc | KeyCode::Left | KeyCode::Backspace)
                if !(category == SettingCategory::Agents
                    && view.selected < LIMIT_ROWS
                    && key.code == KeyCode::Left) =>
            {
                view.selected = SettingCategory::ALL
                    .iter()
                    .position(|item| *item == category)
                    .unwrap_or(0);
                view.category = None;
                view.top = 0;
            }
            (
                Some(SettingCategory::Agents),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('-' | '+'),
            ) if view.selected < LIMIT_ROWS => {
                let more = matches!(key.code, KeyCode::Right | KeyCode::Char('+'));
                let limit = if more {
                    view.max_agents_at_work.saturating_add(1)
                } else {
                    view.max_agents_at_work.saturating_sub(1).max(1)
                };
                return Action::SetAgentLimit(limit);
            }
            (Some(SettingCategory::Agents), KeyCode::Enter)
                if view.agent_file(view.selected).is_some() =>
            {
                if let Some(name) = view.agent_file(view.selected) {
                    return Action::OpenAgentFile(name);
                }
            }
            (Some(SettingCategory::Skills), KeyCode::Enter) => {
                if let Some(skill) = view.skills.get(view.selected) {
                    return Action::OpenSkill(skill.name.clone());
                }
            }
            (Some(SettingCategory::Mcp), KeyCode::Enter) => {
                if let Some(server) = view.mcp.get(view.selected) {
                    return Action::OpenMcp(Some(server.name.clone()));
                }
            }
            (Some(SettingCategory::Mcp), KeyCode::Char('n')) => return Action::OpenMcp(None),
            (Some(SettingCategory::Mcp), KeyCode::Char('c')) => {
                if let Some(server) = view.mcp.get(view.selected) {
                    return Action::RestartMcp(server.name.clone());
                }
            }
            (Some(_), KeyCode::Char(' ') | KeyCode::Enter) => {
                if let Some(action) = view.switch(view.selected) {
                    return action;
                }
            }
            (Some(_), KeyCode::Char('r')) => return Action::Command(Command::Settings),
            _ => {}
        }
        Action::None
    }

    pub fn settings_paste(&mut self, pasted: &str) {
        if let Some(Panel::Settings(view)) = &mut self.panel
            && let Some(editor) = &mut view.editor
        {
            editor.text.insert(pasted);
        }
    }

    pub fn settings_mouse(&mut self, mouse: MouseEvent) -> Action {
        let Some(Panel::Settings(view)) = &mut self.panel else {
            return Action::None;
        };
        let (x, y) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                match &mut view.editor {
                    Some(editor) => move_rows(&mut editor.text, WHEEL, down),
                    None => {
                        let count = view.len();
                        for _ in 0..WHEEL {
                            step(&mut view.selected, count, down);
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = view
                    .hits
                    .iter()
                    .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
                    .map(|hit| hit.target);
                match hit {
                    Some(HitTarget::Button(button)) => return press(view, button),
                    Some(HitTarget::Switch(row)) => {
                        view.selected = row;
                        return view.switch(row).unwrap_or(Action::None);
                    }
                    Some(HitTarget::Row(row)) => return click_row(view, row),
                    None => {}
                }
            }
            _ => {}
        }
        Action::None
    }
}

/// A click opens a category or a file, or selects a row; a second click on a selected
/// row switches it.
fn click_row(view: &mut SettingsView, row: usize) -> Action {
    match view.category {
        None => {
            view.category = SettingCategory::ALL.get(row).copied();
            view.selected = 0;
            view.top = 0;
        }
        Some(category) if opens_file(view, category, row) => {
            view.selected = row;
            let unsaved = view.editor.as_ref().is_some_and(FileEditor::changed);
            if let Some(editor) = view.editor.as_mut().filter(|_| unsaved) {
                editor.error = Some("save or revert this one first".into());
                return Action::None;
            }
            return match category {
                SettingCategory::Agents => view.agent_file(row).map(Action::OpenAgentFile),
                SettingCategory::Skills => view
                    .skills
                    .get(row)
                    .map(|skill| Action::OpenSkill(skill.name.clone())),
                _ => view
                    .mcp
                    .get(row)
                    .map(|server| Action::OpenMcp(Some(server.name.clone()))),
            }
            .unwrap_or(Action::None);
        }
        Some(_) if view.selected == row => return view.switch(row).unwrap_or(Action::None),
        Some(_) => view.selected = row,
    }
    Action::None
}

/// Whether a click on `row` opens a file beside the list rather than selecting it.
fn opens_file(view: &SettingsView, category: SettingCategory, row: usize) -> bool {
    match category {
        SettingCategory::Agents => view.agent_file(row).is_some(),
        SettingCategory::Skills | SettingCategory::Mcp => true,
        SettingCategory::Tools | SettingCategory::Personalize | SettingCategory::Updates => false,
    }
}

fn press(view: &mut SettingsView, button: FileButton) -> Action {
    let Some(editor) = &mut view.editor else {
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
            view.editor = None;
            Action::None
        }
    }
}

fn move_rows(text: &mut Input, rows: usize, down: bool) {
    for _ in 0..rows {
        if !text.vertical(down) {
            break;
        }
    }
}
