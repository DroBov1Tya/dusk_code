//! The rows of each category of the settings list.

use super::lines::{
    agent_row, bytes, described, indented, manual_context_line, mcp_row, mcp_tool_row,
    memory_size_row, note, right, skill_row, switch_row,
};
use super::{Row, SettingsWindow};
use crate::app::{McpRow, SettingCategory};
use crate::ui::windows::{
    theme::{ACCENT, DUSK, FAILED, HAIRLINE, NEGATIVE, OFF, ON, TEXT, TEXT_SOFT, bold, dim, fg},
    wrap,
};
use dusk_protocol::{
    config::{ApprovalPolicy, ContextPolicy, ContextPreset},
    release::Channel,
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

impl SettingsWindow<'_> {
    pub(super) fn rows(&self, width: usize) -> Vec<Row> {
        match self.view.category {
            None => SettingCategory::ALL
                .iter()
                .map(|category| Row {
                    lines: vec![right(
                        vec![Span::raw(format!("  {}", category.name()))],
                        Span::styled("open > ", dim()),
                        width,
                    )],
                    switch: None,
                    lead: 0,
                })
                .collect(),
            Some(SettingCategory::Models) => self.model_rows(width),
            Some(SettingCategory::Agents) => self.agent_rows(width),
            Some(SettingCategory::Tools) => self.tool_rows(width),
            Some(SettingCategory::Personalize) => {
                let about = "    keeps what you ask it to remember in MEMORY.md; the size is \
                             memory.max_chars in config.yml";
                vec![
                    described(
                        switch_row(vec![Span::raw("  memory")], self.view.memory.enabled, width),
                        about,
                        width,
                    ),
                    memory_size_row(
                        "  memory size, characters",
                        self.view.memory.max_chars,
                        width,
                    ),
                    described(
                        switch_row(
                            vec![Span::raw("  project memory")],
                            self.view.work_history.enabled,
                            width,
                        ),
                        "    facts of the project in this folder, apart from your own memory",
                        width,
                    ),
                    memory_size_row(
                        "  project memory size, characters",
                        self.view.work_history.max_chars,
                        width,
                    ),
                ]
            }
            Some(SettingCategory::Limits) => self.limit_rows(width),
            Some(SettingCategory::VersionControl) => {
                let shown = match self.view.branch_prefix.as_str() {
                    "" => "none".to_owned(),
                    prefix => prefix.to_owned(),
                };
                let mut lines = vec![right(
                    vec![Span::raw("  branch prefix")],
                    Span::styled(format!("{shown}  "), fg(TEXT)),
                    width,
                )];
                if let Some(input) = &self.view.branch_input {
                    lines.push(manual_context_line(input, width));
                }
                let row = Row {
                    lines,
                    switch: None,
                    lead: 0,
                };
                let about =
                    "    the start of the branch names it creates, such as dusk/; empty adds none";
                vec![described(row, about, width)]
            }
            Some(SettingCategory::Appearance) => {
                let minutes = match self.view.idle_screen_minutes {
                    0 => "off".to_owned(),
                    minutes => format!("{minutes} min"),
                };
                let line = right(
                    vec![Span::raw("  idle screen")],
                    Span::styled(format!("\u{2212} {minutes} +  "), fg(TEXT)),
                    width,
                );
                let row = Row {
                    lines: vec![line],
                    switch: None,
                    lead: 0,
                };
                let about = "    an animation after this long with no input and no work; 0 is off";
                vec![described(row, about, width)]
            }
            Some(SettingCategory::System) => self.system_rows(width),
            Some(SettingCategory::Access) => {
                let full = "    on: no questions; off: asks before outside paths and commands";
                let sandbox = match self.view.sandbox {
                    true => {
                        "    on: commands run without asking, writing only in the working \
                         folders; outside paths are still asked"
                    }
                    false => "    this system has no sandbox for commands",
                };
                let logs = "    off: no log files and no traces are written or sent, from now on";
                let policy = |policy: ApprovalPolicy, about: &'static str| {
                    let label = format!("  {}", policy.name());
                    described(
                        switch_row(vec![Span::raw(label)], self.view.approvals == policy, width),
                        about,
                        width,
                    )
                };
                vec![
                    policy(ApprovalPolicy::FullAccess, full),
                    policy(ApprovalPolicy::Sandbox, sandbox),
                    policy(
                        ApprovalPolicy::Arbiter,
                        "    on: an agent decides each request in your place and keeps a thread \
                         of its verdicts, open from the sidebar",
                    ),
                    described(
                        switch_row(vec![Span::raw("  logs")], self.view.logging, width),
                        logs,
                        width,
                    ),
                ]
            }
            Some(SettingCategory::Updates) => {
                let dev = self.view.update_channel == Channel::Dev;
                let about = "    on: every tag, pre-releases too; off: full releases only. \
                             /update and duskcode --update follow it";
                vec![described(
                    switch_row(vec![Span::raw("  dev channel")], dev, width),
                    about,
                    width,
                )]
            }
            Some(SettingCategory::Skills) => {
                let mut rows: Vec<Row> = self
                    .view
                    .skills
                    .iter()
                    .map(|skill| skill_row(skill, self.view.editor.is_none(), width))
                    .collect();
                if rows.is_empty() {
                    rows.push(note("  no skills: add folders with a SKILL.md to skills/"));
                }
                rows
            }
            Some(SettingCategory::Mcp) => {
                let mut rows: Vec<Row> = self
                    .view
                    .mcp_rows()
                    .into_iter()
                    .map(|row| match row {
                        McpRow::Server(server) => {
                            mcp_row(server, self.view.mcp_open.contains(&server.name), width)
                        }
                        McpRow::Tool(_, tool) => mcp_tool_row(tool, width),
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(note("  no MCP servers: press n to add one"));
                }
                rows
            }
        }
    }

    /// Context choices are saved for this provider/model pair; effort uses the shared setting.
    pub(super) fn model_rows(&self, width: usize) -> Vec<Row> {
        let policies = [
            ContextPolicy::Auto,
            ContextPolicy::Fixed(ContextPreset::K128),
            ContextPolicy::Fixed(ContextPreset::K256),
            ContextPolicy::Fixed(ContextPreset::K512),
            ContextPolicy::Fixed(ContextPreset::M1),
        ];
        let names = ["Auto (the model's window)", "128k", "256k", "512k", "1M"];
        let mut rows: Vec<Row> = policies
            .into_iter()
            .zip(names)
            .map(|(policy, name)| {
                let active = self.view.context_policy == Some(policy);
                Row {
                    lines: vec![Line::from(vec![
                        Span::styled(
                            format!("  {} ", if active { ON } else { OFF }),
                            fg(if active { ACCENT } else { HAIRLINE }),
                        ),
                        Span::styled(
                            format!("context {name}"),
                            if active { fg(TEXT) } else { Style::new() },
                        ),
                    ])],
                    switch: None,
                    lead: 0,
                }
            })
            .collect();
        let manual = match &self.view.manual_context {
            Some(_) => "manual tokens:".into(),
            None => match self.view.context_policy {
                Some(ContextPolicy::Manual(tokens)) => format!("manual: {tokens} tokens"),
                _ => "manual token count...".into(),
            },
        };
        let active = matches!(self.view.context_policy, Some(ContextPolicy::Manual(_)));
        let mut manual_lines = vec![Line::from(vec![
            Span::styled(
                format!("  {} ", if active { ON } else { OFF }),
                fg(if active { ACCENT } else { HAIRLINE }),
            ),
            Span::styled(
                format!("context {manual}"),
                fg(if self.view.manual_context.is_some() {
                    ACCENT
                } else {
                    TEXT_SOFT
                }),
            ),
        ])];
        if let Some(input) = &self.view.manual_context {
            manual_lines.push(manual_context_line(input, width));
        }
        rows.push(Row {
            lines: manual_lines,
            switch: None,
            lead: 0,
        });
        rows.push(Row {
            lines: vec![Line::from(vec![
                Span::raw("  reasoning effort  "),
                Span::styled(
                    self.view.effort.map_or("default", |effort| effort.name()),
                    fg(TEXT),
                ),
                Span::styled("  open > (shared across models)", dim()),
            ])],
            switch: None,
            lead: 0,
        });
        let model = self
            .view
            .selected_model
            .as_deref()
            .unwrap_or("no model selected; use /model");
        let mut heading = vec![
            Line::from(Span::styled(
                format!("  {}", wrap::clip(model, width.saturating_sub(2))),
                bold(TEXT),
            )),
            Line::from(Span::styled(
                format!(
                    "  effective input limit: {} tokens (model window is the ceiling)",
                    self.view.effective_limit
                ),
                fg(TEXT_SOFT),
            )),
        ];
        if let Some(error) = &self.view.model_error {
            heading.extend(indented(error, fg(NEGATIVE), "  ", width));
        }
        heading.push(Line::default());
        rows[0].lead = heading.len();
        heading.append(&mut rows[0].lines);
        rows[0].lines = heading;
        rows
    }

    /// How many agents work at once and how far a goal goes by itself.
    pub(super) fn limit_rows(&self, width: usize) -> Vec<Row> {
        let row = |name: &str, value: String, about: &str| {
            let line = right(
                vec![Span::raw(format!("  {name}"))],
                Span::styled(format!("\u{2212} {value} +  "), fg(TEXT)),
                width,
            );
            let row = Row {
                lines: vec![line],
                switch: None,
                lead: 0,
            };
            described(row, about, width)
        };
        vec![
            row(
                "concurrent agents",
                self.view.max_agents_at_work.to_string(),
                "    subagents at work across all threads",
            ),
            row(
                "goal rounds",
                self.view.max_goal_rounds.to_string(),
                "    turns a goal starts by itself before it pauses",
            ),
        ]
    }

    /// The limit first, then the tree, then what is wrong with the agents folder.
    pub(super) fn agent_rows(&self, width: usize) -> Vec<Row> {
        let mut rows = vec![Row {
            lines: vec![right(
                vec![Span::raw("  concurrent agents")],
                Span::styled(
                    format!("\u{2212} {} +  ", self.view.max_agents_at_work),
                    fg(TEXT),
                ),
                width,
            )],
            switch: None,
            lead: 0,
        }];
        rows.push(Row {
            lines: vec![right(
                vec![Span::raw("  goal rounds before a pause")],
                Span::styled(
                    format!("\u{2212} {} +  ", self.view.max_goal_rounds),
                    fg(TEXT),
                ),
                width,
            )],
            switch: None,
            lead: 0,
        });
        rows.extend(self.view.agents.iter().map(|agent| agent_row(agent, width)));
        for problem in &self.view.problems {
            rows.push(Row {
                lines: vec![Line::from(Span::styled(
                    format!("  {FAILED} {}: {}", problem.node, problem.reason),
                    fg(NEGATIVE),
                ))],
                switch: None,
                lead: 0,
            });
        }
        for skipped in &self.view.skipped {
            rows.push(note(&format!("  {}: {}", skipped.node, skipped.reason)));
        }
        if self.view.agents.len() <= 2 {
            rows.push(note(
                "  add folders with an AGENT.md to agents/ for expert agents",
            ));
        }
        rows
    }

    /// `left`, clipped to leave room for an on/off switch at the right edge.
    /// Tools grouped by kind, each group under its name.
    pub(super) fn tool_rows(&self, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut kind = None;
        for tool in &self.view.tools {
            let mut row = switch_row(
                vec![Span::raw(format!("    {}", tool.name))],
                tool.enabled,
                width,
            );
            if kind != Some(tool.kind) {
                kind = Some(tool.kind);
                let gap = if rows.is_empty() {
                    vec![]
                } else {
                    vec![Line::default()]
                };
                row.lead = gap.len() + 1;
                let heading =
                    Line::from(Span::styled(format!("  {}", tool.kind.name()), bold(DUSK)));
                row.lines = [gap, vec![heading], row.lines].concat();
            }
            rows.push(row);
        }
        rows
    }

    /// The process first, then what the records take on disk.
    pub(super) fn system_rows(&self, width: usize) -> Vec<Row> {
        let Some((report, _)) = &self.view.system else {
            return vec![note("  reading...")];
        };
        let value = |left: &str, value: String| Row {
            lines: vec![right(
                vec![Span::raw(format!("    {left}"))],
                Span::styled(format!("{value}  "), fg(TEXT)),
                width,
            )],
            switch: None,
            lead: 0,
        };
        let heading = |text: &str| Row {
            lines: vec![
                Line::default(),
                Line::from(Span::styled(format!("  {text}"), bold(DUSK))),
            ],
            switch: None,
            lead: 0,
        };
        let mut rows = vec![heading("application")];
        match report.process {
            Some(process) => {
                rows.push(value("memory in use", bytes(process.resident_bytes)));
                rows.push(value(
                    "processor",
                    format!("{:.1}% of a core", process.cpu_percent),
                ));
            }
            None => rows.push(note("    ps is not available here")),
        }
        let database = report.database_bytes + report.wal_bytes;
        rows.push(heading("records on disk"));
        rows.push(value("database", bytes(database)));
        rows.push(value("  of it, write-ahead log", bytes(report.wal_bytes)));
        rows.push(value("image cards", bytes(report.artifacts_bytes)));
        rows.push(value("logs", bytes(report.logs_bytes)));
        rows.push(value(
            "threads / messages",
            format!("{} / {}", report.threads, report.messages),
        ));
        rows
    }
}
