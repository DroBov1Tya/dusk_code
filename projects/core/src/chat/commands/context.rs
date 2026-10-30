//! `/context`: what the next request of a thread carries, part by part: the system
//! prompt, the summary of compacted history, every tool definition, and the messages,
//! with tool results counted by tool. Before a thread exists, MAIN.md and the tools.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::{
    providers::ToolSpec,
    runtime::chat::{Conversation, spec_tokens},
    state::{MessageRole, State, estimate_tokens},
};

pub struct ContextReport {
    /// The size of the next request, as the sidebar counts it.
    pub total: u64,
    pub limit: u64,
    pub sections: Vec<ContextSection>,
}

/// A part of the request; its tokens are estimated, the total may be the provider's.
pub struct ContextSection {
    pub name: &'static str,
    pub tokens: u64,
    /// What it is made of, largest first.
    pub items: Vec<(String, u64)>,
}

pub async fn report(
    state: &State,
    thread_id: Option<&str>,
    main_prompt: Option<String>,
    specs: &[ToolSpec],
    limit: u64,
) -> Result<ContextReport> {
    let mut sections = Vec::new();
    let conversation = match thread_id {
        Some(thread_id) => Some(Conversation::load(state, thread_id).await?),
        None => None,
    };
    match &conversation {
        Some(conversation) => sections.extend(thread_sections(conversation)),
        None => {
            let tokens = main_prompt.as_deref().map_or(0, estimate_tokens);
            sections.push(section("system prompt", vec![("MAIN.md".into(), tokens)]));
        }
    }
    let tools = specs
        .iter()
        .map(|spec| (spec.name.clone(), spec_tokens(std::slice::from_ref(spec))))
        .collect();
    sections.insert(1.min(sections.len()), section("tools", tools));
    let estimated = sections.iter().map(|section| section.tokens).sum();
    let total = conversation.map_or(estimated, |conversation| conversation.tokens(specs));
    Ok(ContextReport {
        total,
        limit,
        sections,
    })
}

fn thread_sections(conversation: &Conversation) -> Vec<ContextSection> {
    let mut system = Vec::new();
    let mut messages: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    let mut results: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut tool_names = BTreeMap::new();
    for message in &conversation.messages {
        let tokens = message.context_tokens();
        for call in &message.tool_calls {
            tool_names.insert(call.id.clone(), call.name.clone());
        }
        let kind = match message.role {
            MessageRole::System => {
                system.push((first_line(&message.content), tokens));
                continue;
            }
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
    let mut sections = vec![section("system prompt", system)];
    if let Some(compaction) = &conversation.compaction {
        let item = format!("messages 1 to {}", compaction.through_sequence);
        sections.push(section(
            "summary of earlier messages",
            vec![(item, compaction.summary_tokens)],
        ));
    }
    let messages = messages
        .into_iter()
        .map(|(kind, counted)| (kind.to_owned(), counted))
        .collect();
    sections.push(section("messages", counted(messages)));
    sections.push(section("tool results", counted(results.into_iter().collect())));
    sections
}

fn section(name: &'static str, mut items: Vec<(String, u64)>) -> ContextSection {
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ContextSection {
        name,
        tokens: items.iter().map(|(_, tokens)| tokens).sum(),
        items,
    }
}

/// A system message by its first words, as a label.
fn first_line(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("");
    let label: String = line.trim().trim_start_matches('#').trim().chars().take(48).collect();
    if label.is_empty() { "system message".into() } else { label }
}
