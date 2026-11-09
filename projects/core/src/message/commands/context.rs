//! `/context`: what the next request of a thread carries, part by part: the system
//! prompt, the summary of compacted history, every tool definition, and the messages,
//! with tool results counted by tool. Before a thread exists, the
//! mode's prompt and the tools. `detail` adds the per-item breakdown the GUI shows:
//! skills, MCP tools by server, memory files and MCP instructions, each with its usage
//! counts, and the per-source totals built from it.

use std::collections::BTreeMap;

use anyhow::Result;
pub use dusk_protocol::context::{ContextItemDetail, ContextReport, ContextSection, ContextSource};

use crate::{
    providers::ToolSpec,
    runtime::thread::{Conversation, spec_tokens},
    state::{MessageRole, State, estimate_tokens},
};

/// What the report breaks down per item, gathered by the runtime from what the thread
/// actually gets: the skills it can load, the MCP servers ready for it, and the memory
/// files that ride in every request.
#[derive(Debug, Default)]
pub struct ContextDetail {
    /// Skills on now: name and the description listed in `load_skill`.
    pub skills: Vec<(String, String)>,
    pub mcp: Vec<McpServerDetail>,
    /// Memory files: the tool that carries them, the file, and the character cap it is
    /// sent under.
    pub memory: Vec<MemoryFile>,
}

/// One ready MCP server: the tools it gave this thread, and its instructions if any.
#[derive(Debug, Default)]
pub struct McpServerDetail {
    pub server: String,
    /// Model tool names, `server__tool`, the ones of this server among the specs.
    pub tools: Vec<String>,
    pub instructions: Option<String>,
}

/// One memory file as it is sent.
#[derive(Debug)]
pub struct MemoryFile {
    pub label: String,
    pub path: std::path::PathBuf,
    pub max_chars: usize,
}

/// The item kinds of `details`, which set the source category they roll up to.
pub const KIND_SKILL: &str = "skill";
pub const KIND_MCP_TOOL: &str = "mcp tool";
pub const KIND_MEMORY_FILE: &str = "memory file";

pub async fn report(
    state: &State,
    thread_id: Option<&str>,
    mode: &str,
    prompt: Option<String>,
    specs: &[ToolSpec],
    limit: u64,
    detail: &ContextDetail,
) -> Result<ContextReport> {
    let usage = state.item_usage(thread_id).await?;
    let mut sections = Vec::new();
    let conversation = match thread_id {
        Some(thread_id) => Some(Conversation::load(state, thread_id).await?),
        None => None,
    };
    match &conversation {
        Some(conversation) => sections.extend(thread_sections(conversation)),
        None => {
            let tokens = prompt.as_deref().map_or(0, estimate_tokens);
            sections.push(section(
                "system prompt",
                vec![(format!("{mode}.md"), tokens)],
                "system prompt files",
            ));
        }
    }
    let tools = specs
        .iter()
        .map(|spec| (spec.name.clone(), spec_tokens(std::slice::from_ref(spec))))
        .collect();
    let mut tools_section = section_named("tools", tools, "other tools");
    tools_section.details = tool_details(specs, detail, &usage);
    sections.insert(1.min(sections.len()), tools_section);
    let instructions: Vec<(String, u64)> = detail
        .mcp
        .iter()
        .filter_map(|server| {
            let text = server.instructions.as_deref()?;
            Some((server.server.clone(), estimate_tokens(text)))
        })
        .collect();
    if !instructions.is_empty() {
        // Servers whose instructions the core carries; listed per server with cost.
        let mut section = section_named("MCP instructions", instructions, "MCP instructions");
        section.details = detail
            .mcp
            .iter()
            .filter(|server| server.instructions.is_some())
            .map(|server| ContextItemDetail {
                name: server.server.clone(),
                kind: "MCP server".into(),
                tokens: estimate_tokens(server.instructions.as_deref().unwrap_or_default()),
                uses_total: 0,
                uses_in_chat: 0,
            })
            .collect();
        sections.push(section);
    }
    let estimated = sections.iter().map(|section| section.tokens).sum();
    let total = conversation.map_or(estimated, |conversation| conversation.tokens(specs));
    let sources = source_totals(&sections);
    Ok(ContextReport {
        total,
        limit,
        sections,
        sources,
    })
}

fn thread_sections(conversation: &Conversation) -> Vec<ContextSection> {
    let system = conversation
        .system_texts(u64::MAX)
        .into_iter()
        .map(|text| (first_line(&text), estimate_tokens(&text)))
        .collect();
    let mut messages: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    let mut results: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut tool_names = BTreeMap::new();
    for message in &conversation.messages {
        let tokens = message.context_tokens();
        for call in &message.tool_calls {
            tool_names.insert(call.id.clone(), call.name.clone());
        }
        let kind = match message.role {
            MessageRole::System => continue,
            MessageRole::Tool => {
                let name = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| tool_names.get(id).cloned())
                    .unwrap_or_else(|| "unknown tool".into());
                let entry = results.entry(name).or_default();
                *entry = (entry.0 + 1, entry.1 + tokens);
                continue;
            }
            MessageRole::User if message.from_thread_id.is_some() => "from agents",
            MessageRole::User => "from you",
            MessageRole::Assistant => "model answers and tool calls",
            MessageRole::Job => "background job results",
        };
        let entry = messages.entry(kind).or_default();
        *entry = (entry.0 + 1, entry.1 + tokens);
    }
    let counted = |items: Vec<(String, (usize, u64))>| {
        items
            .into_iter()
            .map(|(name, (count, tokens))| (format!("{name} \u{00D7}{count}"), tokens))
            .collect()
    };
    let mut sections = vec![section("system prompt", system, "system prompt files")];
    if let Some(compaction) = &conversation.compaction {
        let item = format!("messages 1 to {}", compaction.through_sequence);
        sections.push(section_named(
            "summary of earlier messages",
            vec![(item, compaction.summary_tokens)],
            "messages",
        ));
    }
    let messages = messages
        .into_iter()
        .map(|(kind, counted)| (kind.to_owned(), counted))
        .collect();
    sections.push(section("messages", counted(messages), "messages"));
    sections.push(section(
        "tool results",
        counted(results.into_iter().collect()),
        "tool results",
    ));
    sections
}

/// Per-item detail for the tools section: what each item is made of, skills and MCP
/// tools and memory files, with their usage counts. Their tokens are parts of the
/// tool's own item, which is why only the parts of a tool that carries them are listed.
fn tool_details(
    specs: &[ToolSpec],
    detail: &ContextDetail,
    usage: &[crate::state::ItemUsage],
) -> Vec<ContextItemDetail> {
    let mut details = Vec::new();
    // Skills ride in the load_skill description as its listing; each line is one item.
    if !detail.skills.is_empty() && specs.iter().any(|spec| spec.name == "load_skill") {
        for (name, description) in &detail.skills {
            let listed = format!(
                "- {}: {}",
                name,
                description.chars().take(400).collect::<String>()
            );
            details.push(detail_for(
                name.clone(),
                KIND_SKILL,
                estimate_tokens(&listed),
                usage,
            ));
        }
    }
    for (name, tokens) in specs
        .iter()
        .filter(|spec| mcp_tool_names(detail).contains(&spec.name))
        .map(|spec| (spec.name.clone(), spec_tokens(std::slice::from_ref(spec))))
    {
        details.push(detail_for(name, KIND_MCP_TOOL, tokens, usage));
    }
    for file in &detail.memory {
        let trimmed = read_capped(file);
        if !trimmed.is_empty() {
            details.push(ContextItemDetail {
                name: file.label.clone(),
                kind: KIND_MEMORY_FILE.into(),
                tokens: estimate_tokens(&trimmed),
                uses_total: 0,
                uses_in_chat: 0,
            });
        }
    }
    details
}

fn mcp_tool_names(detail: &ContextDetail) -> Vec<String> {
    detail.mcp.iter().flat_map(|s| s.tools.clone()).collect()
}

fn read_capped(file: &MemoryFile) -> String {
    // A memory file that is gone or unreadable simply adds nothing to the report.
    let text = std::fs::read_to_string(&file.path).unwrap_or_default();
    text.trim().chars().take(file.max_chars).collect()
}

fn detail_for(
    name: String,
    kind: &str,
    tokens: u64,
    usage: &[crate::state::ItemUsage],
) -> ContextItemDetail {
    let counted = usage
        .iter()
        .find(|usage| usage.kind == kind && usage.name == name);
    ContextItemDetail {
        name,
        kind: kind.to_owned(),
        tokens,
        uses_total: counted.map_or(0, |usage| usage.uses_total),
        uses_in_chat: counted.map_or(0, |usage| usage.uses_in_chat),
    }
}

fn section(name: &str, items: Vec<(String, u64)>, category: &str) -> ContextSection {
    section_named(name, items, category)
}

fn section_named(name: &str, mut items: Vec<(String, u64)>, category: &str) -> ContextSection {
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ContextSection {
        name: name.to_owned(),
        tokens: items.iter().map(|(_, tokens)| tokens).sum(),
        items,
        details: Vec::new(),
        category: category.to_owned(),
    }
}

/// How much each source category contributed, largest first: each detail rolls up to
/// its kind's category, and what is left of a section stays in the section's own one.
fn source_totals(sections: &[ContextSection]) -> Vec<ContextSource> {
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    for section in sections {
        let mut counted = 0;
        for detail in &section.details {
            let category = match detail.kind.as_str() {
                KIND_SKILL => "skills",
                KIND_MCP_TOOL => "MCP tools",
                KIND_MEMORY_FILE => "memory",
                _ => section.category.as_str(),
            };
            *totals.entry(category.to_owned()).or_default() += detail.tokens;
            counted += detail.tokens;
        }
        *totals.entry(section.category.clone()).or_default() +=
            section.tokens.saturating_sub(counted);
    }
    let mut sources: Vec<ContextSource> = totals
        .into_iter()
        .filter(|(_, tokens)| *tokens > 0)
        .map(|(category, tokens)| ContextSource { category, tokens })
        .collect();
    sources.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then_with(|| a.category.cmp(&b.category))
    });
    sources
}

/// A system message by its first words, as a label.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let label: String = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(48)
        .collect();
    if label.is_empty() {
        "system message".into()
    } else {
        label
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testing::{temp_dir, temp_state};

    #[tokio::test]
    async fn the_breakdown_splits_by_source_and_counts_usage() {
        let state = temp_state("context_breakdown").await;
        state.create_thread("chat", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        state
            .add_system_message("chat", "You work alone.")
            .await
            .unwrap();
        state
            .add_user_message("chat", "uid", "hello there")
            .await
            .unwrap();
        state
            .record_item_use(Some("chat"), "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(Some("other"), "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(None, "skill", "debugging")
            .await
            .unwrap();
        state
            .record_item_use(Some("chat"), "mcp tool", "srv__echo")
            .await
            .unwrap();

        let memory_dir = temp_dir("context_memory");
        std::fs::create_dir_all(&memory_dir).unwrap();
        std::fs::write(memory_dir.join("MEMORY.md"), "one two three four five").unwrap();
        let detail = ContextDetail {
            skills: vec![("debugging".into(), "Find the cause first.".into())],
            mcp: vec![McpServerDetail {
                server: "srv".into(),
                tools: vec!["srv__echo".into()],
                instructions: Some("Use echo twice.".into()),
            }],
            memory: vec![MemoryFile {
                label: "remember".into(),
                path: memory_dir.join("MEMORY.md"),
                max_chars: 5000,
            }],
        };
        let specs = vec![
            ToolSpec {
                name: "srv__echo".into(),
                description: "Echo it back.".into(),
                parameters: json!({"type": "object"}),
            },
            ToolSpec {
                name: "load_skill".into(),
                description: "Load a skill.\nSkills:\n- debugging: Find the cause first.".into(),
                parameters: json!({"type": "object"}),
            },
            ToolSpec {
                name: "remember".into(),
                description: "The memory as it is: one two three four five".into(),
                parameters: json!({"type": "object"}),
            },
        ];
        let report = report(&state, Some("chat"), "main", None, &specs, 100_000, &detail)
            .await
            .unwrap();

        let tools = report.sections.iter().find(|s| s.name == "tools").unwrap();
        let skill = tools.details.iter().find(|d| d.kind == KIND_SKILL).unwrap();
        assert_eq!(skill.name, "debugging");
        assert!(skill.tokens > 0);
        assert_eq!(skill.uses_total, 3, "the total spans all threads");
        assert_eq!(skill.uses_in_chat, 1);
        let mcp = tools
            .details
            .iter()
            .find(|d| d.kind == KIND_MCP_TOOL)
            .unwrap();
        assert_eq!(mcp.name, "srv__echo");
        assert_eq!(mcp.uses_total, 1);
        assert!(tools.details.iter().any(|d| d.kind == KIND_MEMORY_FILE));

        let instructions = report
            .sections
            .iter()
            .find(|s| s.name == "MCP instructions")
            .unwrap();
        assert_eq!(instructions.items[0].0, "srv");

        let category = |name: &str| {
            report
                .sources
                .iter()
                .find(|source| source.category == name)
                .map(|source| source.tokens)
                .unwrap_or(0)
        };
        for name in [
            "MCP tools",
            "skills",
            "memory",
            "MCP instructions",
            "messages",
        ] {
            assert!(category(name) > 0, "{name} contributed nothing");
        }
        let total: u64 = report.sources.iter().map(|s| s.tokens).sum();
        let sections: u64 = report.sections.iter().map(|s| s.tokens).sum();
        assert_eq!(total, sections, "the source totals add up to the sections");
    }
}
