//! The agent's steps: a run of them with the latest in sight, and one step unfolded to
//! its arguments and answer.

use dusk_protocol::thread::{Tool, ToolState};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{
    RUN, STEP,
    activity::{disclosure, disclosure_icon, fold_foot, long, step_muted},
};
use crate::{
    pages::chat::transcript::Item,
    ui::{Type, widgets},
    workspace::Workspace,
};

fn visible_tool(tool: &Tool) -> bool {
    !matches!(tool.name.as_str(), "display_image" | "display_audio")
        || tool.state != ToolState::Done
}

/// The steps a run shows: a picture card stands for the step that showed it.
pub fn steps(items: &[Item]) -> Vec<&Tool> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Tool(tool) if visible_tool(tool) => Some(tool),
            _ => None,
        })
        .collect()
}

/// A run of steps: the latest one, and the count of the others to unfold.
pub fn run(
    ws: &Workspace,
    shown: &str,
    start: usize,
    tools: &[&Tool],
    cx: &mut Context<Workspace>,
) -> Div {
    let open = ws.unfolded.contains(&(shown.to_owned(), start | RUN));
    let mut block = div().flex().flex_col().gap_1();
    if tools.len() > 1 {
        let id = shown.to_owned();
        let label = if open {
            widgets::count(tools.len(), "step")
        } else {
            format!("{} earlier", widgets::count(tools.len() - 1, "step"))
        };
        block = block.child(
            disclosure(SharedString::from(format!("run-{shown}-{start}")), open, cx)
                .aria_label(label.clone())
                .child(disclosure_icon(open, step_muted(cx)))
                .child(label)
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | RUN, cx))),
        );
    }
    let first = if open { 0 } else { tools.len() - 1 };
    for (offset, tool) in tools.iter().enumerate().skip(first) {
        block = block.child(step(ws, shown, start + offset, tool, cx));
    }
    if open && tools.len() > 4 {
        let id = shown.to_owned();
        block = block.child(
            fold_foot(
                SharedString::from(format!("run-foot-{shown}-{start}")),
                widgets::count(tools.len(), "step"),
                cx,
            )
            .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, start | RUN, cx))),
        );
    }
    block
}

/// What a step is, at a glance, before its words are read: the terminal, a file, a
/// search, an edit, the web, another agent, a connector's tool. The tool's name says it;
/// a connector's tool is named `server__tool`.
fn step_icon(tool: &str) -> IconName {
    use dusk_protocol::agent::Doing;
    let has = |words: &[&str]| words.iter().any(|word| tool.contains(word));
    if dusk_protocol::tools::is_agent_tool(tool) {
        IconName::Bot
    } else if tool.contains("__") {
        IconName::Network
    } else if has(&["screenshot", "click", "type_text", "computer", "display"]) {
        IconName::Frame
    } else if has(&["fetch", "web", "navigate", "browse", "url"]) {
        IconName::Globe
    } else if has(&["skill"]) {
        IconName::BookOpen
    } else if has(&["remember", "memory"]) {
        IconName::MemoryStick
    } else {
        match Doing::of_tool(tool) {
            Doing::Searching => IconName::Search,
            Doing::Reading => IconName::FileText,
            Doing::Planning => IconName::Replace,
            _ => IconName::SquareTerminal,
        }
    }
}

/// The model of the subagent an agent tool's call started, when the core said it.
fn agent_model(ws: &Workspace, shown: &str, tool: &Tool) -> Option<String> {
    if !dusk_protocol::tools::is_agent_tool(&tool.name) {
        return None;
    }
    ws.chats
        .get(shown)?
        .agents
        .values()
        .find(|agent| agent.call_id == tool.call_id)?
        .model
        .clone()
}

pub fn step(
    ws: &Workspace,
    shown: &str,
    index: usize,
    tool: &Tool,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let muted = step_muted(cx);
    let surface = theme.muted;
    let open = ws.unfolded.contains(&(shown.to_owned(), index | STEP));
    // The icon says what the step is; its color, how it stands.
    let icon = step_icon(&tool.name);
    let (color, status) = match tool.state {
        ToolState::Running => (theme.warning, "Running"),
        ToolState::Done => (muted, "Completed"),
        ToolState::Failed => (theme.danger, "Failed"),
    };
    let id = shown.to_owned();
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let detail = tool.detail.trim().to_owned();
    // Said as a person would; the tool's own name shows once the step is open.
    let summary = if tool.name == "load_skill" {
        "Skill"
    } else {
        dusk_protocol::tools::phrase(&tool.name, &tool.call_id)
    };
    // Work handed to another agent is the turn's big move: it carries the accent.
    let lead = if dusk_protocol::tools::is_agent_tool(&tool.name) {
        theme.primary
    } else {
        theme.foreground
    };
    let button = disclosure(
        SharedString::from(format!("step-{shown}-{index}")),
        open,
        cx,
    )
    .flex_1()
    .aria_label(format!("{summary}, {}, {status}", tool.name))
    .child(widgets::icon(icon, color))
    .child(div().flex_none().text_color(lead).child(summary))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .meta()
            .text_color(muted)
            .child(detail),
    )
    // The model the agent answers on, beside the work handed to it.
    .children(agent_model(ws, shown, tool).map(|model| {
        div()
            .flex_none()
            .max_w(relative(0.30))
            .truncate()
            .meta()
            .text_color(muted)
            .child(model)
    }))
    .when(open, |row| {
        row.child(
            div()
                .max_w(relative(0.30))
                .truncate()
                .meta()
                .font_family(mono.clone())
                .child(tool.name.clone()),
        )
    })
    .when(tool.state == ToolState::Failed, |row| {
        row.child(div().meta().text_color(color).child(status))
    })
    .child(disclosure_icon(open, muted))
    .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index | STEP, cx)));
    let mut row = div().flex().items_center().gap_2().child(button);
    // A long command can go on in the background while the agent works on.
    if tool.state == ToolState::Running && tool.name == "run_command" {
        row = row.child(background_button(shown, index, tool, cx));
    }
    let mut line = div().flex().flex_col().child(row);
    if open {
        let block = |title: &'static str, text: &str| {
            let group = SharedString::from(format!("step-{title}-{shown}-{index}"));
            div()
                .group(group.clone())
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().meta().text_color(muted).child(title))
                        .child(widgets::copy(
                            SharedString::from(format!("step-copy-{title}-{shown}-{index}")),
                            &group,
                            text,
                        )),
                )
                .child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(surface)
                        .meta()
                        .font_family(mono.clone())
                        .child(text.to_owned()),
                )
        };
        let mut details = div()
            .flex()
            .flex_col()
            .gap_2()
            .pl_4()
            .pt_1()
            .child(block("Arguments", &tool.arguments));
        if !tool.output.is_empty() {
            details = details.child(block("Answer", &tool.output));
        }
        line = line.child(widgets::unfold(
            SharedString::from(format!("unfold-step-{shown}-{index}")),
            details,
        ));
        if long(&tool.arguments) || long(&tool.output) {
            let id = shown.to_owned();
            line = line.child(
                fold_foot(
                    SharedString::from(format!("step-foot-{shown}-{index}")),
                    summary,
                    cx,
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.fold(&id, index | STEP, cx))),
            );
        }
    }
    line
}

pub fn background_button(
    shown: &str,
    index: usize,
    tool: &Tool,
    cx: &mut Context<Workspace>,
) -> Button {
    let (thread_id, call_id) = (shown.to_owned(), tool.call_id.clone());
    Button::new(SharedString::from(format!("background-{shown}-{index}")))
        .ghost()
        .small()
        .label("To background")
        .tooltip("The agent goes on; the result comes when it ends")
        .on_click(cx.listener(move |ws, _, _, cx| {
            cx.stop_propagation();
            ws.send_to_background(&thread_id, &call_id, cx);
        }))
}

impl Workspace {
    fn send_to_background(&mut self, thread_id: &str, call_id: &str, cx: &mut Context<Self>) {
        let (thread_id, call_id) = (thread_id.to_owned(), call_id.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .send_to_background(&thread_id, &call_id)
                    .await
            },
            |ws, result, cx| match result {
                Ok(()) => ws.refresh_inspector(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
