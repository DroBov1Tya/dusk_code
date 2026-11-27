//! The chat's subagents as a stage: each kind of work has its zone (engineering, security,
//! testing and review, infrastructure, the rest), and in it every agent stands as its 8-bit
//! figure with a bubble that says what it does this moment: thinking, writing, or the tool
//! it runs. A figure that works moves; one that is done rests. A click opens its chat.

use crate::ui::press::Press;
use dusk_protocol::{
    system::now_ms,
    thread::{AgentState, clock_words},
};
use std::{cell::RefCell, collections::HashMap};

use gpui_kit::{
    component::{
        ActiveTheme, Colorize, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use dusk_protocol::agent::Zone;

use crate::{
    columns::inspector::mascot,
    pages::chat::transcript::Agent,
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

pub fn render(ws: &Workspace, thread_id: &str, wide: bool, cx: &mut Context<Workspace>) -> Div {
    let Some(chat) = ws
        .chats
        .get(thread_id)
        .filter(|chat| !chat.agents.is_empty())
    else {
        return div().p_5().child(crate::ui::empty(
            IconName::Bot,
            "No subagents yet. When the agent asks others for help, they appear here at work.",
            None,
            cx,
        ));
    };
    let theme = cx.theme();
    let muted = theme.foreground.mix_oklab(theme.muted_foreground, 0.16);
    let (signal, surface) = (theme.warning, theme.muted);
    let working = chat.agents.values().filter(|agent| agent.working()).count();
    let mut zones: Vec<(Zone, Vec<(&String, &Agent)>)> =
        Zone::ALL.iter().map(|zone| (*zone, Vec::new())).collect();
    for (id, agent) in &chat.agents {
        let own = Zone::of(&agent.role);
        if let Some((_, members)) = zones.iter_mut().find(|(zone, _)| *zone == own) {
            members.push((id, agent));
        }
    }
    zones.retain(|(_, members)| !members.is_empty());
    // One zone alone takes the whole width: half a window of it left the text in a strip.
    let columns = if wide && zones.len() > 1 { 2 } else { 1 };
    let mut stage = div().grid().grid_cols(columns).gap_3();
    for (name, mut members) in zones {
        // Those at work first, then the oldest.
        members.sort_by_key(|(_, agent)| (!agent.working(), agent.started_at));
        let busy = members.iter().filter(|(_, agent)| agent.working()).count();
        let mut card = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_lg()
            .bg(surface)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .group_name()
                            .truncate()
                            .child(name.name()),
                    )
                    .child(
                        div()
                            .meta()
                            .text_color(if busy > 0 { signal } else { muted })
                            .child(if busy > 0 {
                                format!("{busy} active")
                            } else {
                                widgets::count(members.len(), "agent")
                            }),
                    ),
            );
        for (id, agent) in members {
            let progress = chat
                .agent_chats
                .get(id.as_str())
                .map(|log| progress(&ws.agent_steps, id, log));
            let asked = chat.approval_waits_on(id);
            card = card.child(figure(id, agent, progress, asked, cx));
        }
        stage = stage.child(card);
    }
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .body()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(widgets::count(chat.agents.len(), "subagent")),
                )
                .child(
                    div()
                        .small()
                        .text_color(if working > 0 { signal } else { muted })
                        .child(format!("{working} active")),
                )
                .child(div().flex_1())
                .children((working < chat.agents.len()).then(|| {
                    Button::new("agents-clear-finished")
                        .ghost()
                        .small()
                        .label("Clear finished")
                        .tooltip("Take the agents that are done off the list")
                        .on_click(cx.listener(|ws, _, _, cx| ws.clear_agents(None, cx)))
                })),
        )
        .child(stage)
}

/// One agent: its figure, its name, and a bubble with what it does now.
/// How far an agent got: its steps so far and the last thing it said.
struct Progress {
    steps: usize,
    said: Option<String>,
    /// `42% context · 1 compaction`, once known.
    context: Option<String>,
}

fn progress(
    counts: &StepCounts,
    id: &str,
    transcript: &crate::pages::chat::transcript::Log,
) -> Progress {
    use crate::pages::chat::transcript::Item;
    let steps = counts.of(id, transcript);
    // The last answer is near the end; a step between them is passed, not counted.
    let said = transcript.items.iter().rev().find_map(|item| match item {
        Item::Assistant { text, .. } => text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned),
        _ => None,
    });
    Progress {
        steps,
        said,
        context: transcript.context_line(),
    }
}

/// The steps each subagent took, counted once for each shape of its chat: the list is
/// drawn again for every tick of a mascot, and a long chat holds thousands of items.
#[derive(Default)]
pub struct StepCounts(RefCell<HashMap<String, (u64, usize)>>);

impl StepCounts {
    fn of(&self, id: &str, transcript: &crate::pages::chat::transcript::Log) -> usize {
        use crate::pages::chat::transcript::Item;
        let shape = transcript.shape();
        let mut counts = self.0.borrow_mut();
        if let Some(&(seen, steps)) = counts.get(id)
            && seen == shape
        {
            return steps;
        }
        let steps = transcript
            .items
            .iter()
            .filter(|item| matches!(item, Item::Tool(_)))
            .count();
        counts.insert(id.to_owned(), (shape, steps));
        steps
    }
}

fn figure(
    id: &str,
    agent: &Agent,
    progress: Option<Progress>,
    asked: bool,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let muted = theme.foreground.mix_oklab(theme.muted_foreground, 0.16);
    let (signal, success, danger, hover) = (
        theme.warning,
        theme.success,
        theme.danger,
        theme.list_active,
    );
    let color = match agent.state {
        AgentState::Done => success.opacity(0.8),
        AgentState::Failed => danger,
        _ => signal,
    };
    let status = match &agent.state {
        AgentState::Thinking => "Thinking".to_owned(),
        AgentState::Writing => "Writing".to_owned(),
        AgentState::Tool(name) => dusk_protocol::tools::phrase(name, &agent.name).to_owned(),
        AgentState::Done => "Done".to_owned(),
        AgentState::Failed => "Failed".to_owned(),
    };
    let open = id.to_owned();
    div()
        .id(SharedString::from(format!("agents-row-{id}")))
        .focusable_row(cx)
        .role(Role::Button)
        .aria_label(format!("Open {}", agent.name))
        .aria_description(status.clone())
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .p_2()
        .rounded_lg()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .press(hover, cx)
        .child(mascot::render(
            SharedString::from(format!("mascot-{id}")),
            &agent.state,
            color,
            3.0,
        ))
        // What it was asked first, then what it does now, then what it last said.
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .meta()
                                .font_weight(FontWeight::MEDIUM)
                                .child(agent.name.clone()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .meta()
                                .text_color(muted)
                                .child(about(agent)),
                        ),
                )
                .child(div().small().truncate().child(headline(&agent.task)))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .meta()
                                .truncate()
                                .text_color(if agent.state == AgentState::Done {
                                    muted
                                } else {
                                    color
                                })
                                .child(status),
                        )
                        .children(progress.as_ref().filter(|p| p.steps > 0).map(|p| {
                            div()
                                .meta()
                                .flex_none()
                                .text_color(muted)
                                .child(widgets::count(p.steps, "step"))
                        }))
                        .children(progress.as_ref().and_then(|p| p.context.clone()).map(
                            |context| div().meta().flex_none().text_color(muted).child(context),
                        ))
                        .child(
                            div()
                                .meta()
                                .flex_none()
                                .text_color(muted)
                                .child(clock_words(agent.run_ms(now_ms()))),
                        ),
                )
                .children(progress.and_then(|p| p.said).map(|said| {
                    div()
                        .meta()
                        .text_color(muted)
                        .truncate()
                        .child(format!("\u{201C}{said}\u{201D}"))
                })),
        )
        // One agent is stopped or put away from its own row; the row itself opens it. One
        // a request of the user's waits on is not put away before it is answered.
        .children((!asked || agent.working()).then(|| {
            if agent.working() {
                let id = id.to_owned();
                Button::new(SharedString::from(format!("agent-stop-{id}")))
                    .ghost()
                    .small()
                    .icon(IconName::Square)
                    .tooltip("Stop this agent")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        cx.stop_propagation();
                        ws.stop_agent(&id, cx);
                    }))
            } else {
                let id = id.to_owned();
                Button::new(SharedString::from(format!("agent-clear-{id}")))
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("Take it off the list")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        cx.stop_propagation();
                        ws.clear_agents(Some(&id), cx);
                    }))
            }
        }))
        .child(widgets::icon(IconName::ChevronRight, muted))
        .on_click(cx.listener(move |ws, _, window, cx| {
            ws.open_agent_chat(&open, cx);
            ws.modal_focus.focus(window, cx);
        }))
}

/// `backend-engineer · gpt-5.5 · high`: the row is narrow, so the model goes without its
/// provider and the effort without its word; the agent's own chat names both in full.
fn about(agent: &Agent) -> String {
    let model = agent
        .model
        .as_deref()
        .map(dusk_protocol::thread::short_model);
    let effort = agent.effort.map(|effort| effort.name().to_owned());
    [Some(agent.role.clone()), model, effort]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{00B7} ")
}

/// A task in one line: its first line that says something, markdown marks off. A brief
/// is pages long, and the row is for telling the agents apart.
fn headline(task: &str) -> String {
    task.lines()
        .map(|line| line.trim().trim_start_matches(['#', '-', '*', ' ']))
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

impl Workspace {
    /// Asks the core how full the context of each subagent of the shown chat is, or of
    /// agent `only`, when this window heard nothing of it: an agent that worked before
    /// the chat was open here sent its numbers to no one.
    pub fn read_agent_contexts(&mut self, only: Option<&str>, cx: &mut Context<Self>) {
        use dusk_protocol::client::{Answer, Command};
        let Some(parent) = self.shown.clone() else {
            return;
        };
        let Some(chat) = self.chats.get(&parent) else {
            return;
        };
        let unknown: Vec<String> = chat
            .agents
            .keys()
            .filter(|id| only.is_none_or(|only| only == id.as_str()))
            .filter(|id| {
                chat.agent_chats
                    .get(id.as_str())
                    .is_none_or(|log| log.context_unknown())
            })
            .cloned()
            .collect();
        for agent in unknown {
            let (asked, parent) = (agent.clone(), parent.clone());
            self.core.call(
                cx,
                move |client| async move {
                    let thread_id = Some(asked);
                    client.command(Command::Context { thread_id }).await
                },
                move |ws, result, cx| match result {
                    Ok(Answer::Context(report)) => {
                        if let Some(chat) = ws.chats.get_mut(&parent) {
                            let log = chat.agent_chats.entry(agent).or_default();
                            log.context_report(&report);
                            cx.notify();
                        }
                    }
                    Ok(_) => {}
                    // The line is an extra: the list stands without it.
                    Err(error) => tracing::warn!("no context for agent {agent}: {error:#}"),
                },
            );
        }
    }
}
