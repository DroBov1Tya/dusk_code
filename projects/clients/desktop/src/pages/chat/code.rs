//! Buttons in the corner of a code block: Copy for every block, Run for shell code. Run
//! starts at once; the core runs the code in the chat's folder, confined as the chat's
//! tools are, and the output lands in the chat under a bar that fills while it runs.
//! While it runs the button turns, and under the pointer it is an X that stops it.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use gpui_kit::base::text::CodeBlock;
use gpui_kit::{
    component::{
        IconName, Sizable,
        button::{Button, ButtonVariants},
        spinner::Spinner,
    },
    *,
};

use crate::{ui::clipboard, workspace::Workspace};

/// Runs a shell code block in the shown chat's folder.
#[derive(Clone, PartialEq, Default, Debug, Action)]
#[action(namespace = dusk, no_json)]
pub struct RunCode {
    pub command: String,
}

/// Stops a code block's command under way in the shown chat, with all it started.
#[derive(Clone, PartialEq, Default, Debug, Action)]
#[action(namespace = dusk, no_json)]
pub struct StopCode {
    pub run: u64,
}

/// The code blocks' commands under way, beside the window's state so a block can read it
/// while it is laid out: by chat and command, the number of the run.
#[derive(Default)]
struct Running {
    /// The chat on screen, whose blocks are the ones drawn.
    shown: Option<String>,
    runs: HashMap<(String, String), u64>,
}

impl Global for Running {}

/// The shown chat changed: its blocks show its own runs.
pub fn show(shown: Option<&String>, cx: &mut App) {
    if cx
        .try_global::<Running>()
        .map(|running| running.shown.as_ref())
        != Some(shown)
    {
        cx.default_global::<Running>().shown = shown.cloned();
    }
}

fn running(command: &str, cx: &App) -> Option<u64> {
    let running = cx.try_global::<Running>()?;
    let key = (running.shown.clone()?, command.to_owned());
    running.runs.get(&key).copied()
}

const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "shell", "console", "fish"];

pub fn actions(block: &CodeBlock, _: &mut Window, cx: &mut App) -> Div {
    let code = block.code().to_string();
    let shell = block
        .lang()
        .is_some_and(|lang| SHELLS.contains(&lang.to_lowercase().as_str()));
    let copy = {
        let text = code.clone();
        // One id per block, so only the pressed one shows the check mark.
        let mut hasher = DefaultHasher::new();
        code.hash(&mut hasher);
        let id = SharedString::from(format!("copy-code-{:x}", hasher.finish()));
        Button::new(id.clone())
            .ghost()
            .small()
            .icon(clipboard::icon(&id, cx))
            .tooltip("Copy")
            .on_click(move |_, _, cx| clipboard::copy(&id, text.clone(), cx))
    };
    let run = shell.then(|| {
        let command = shell_lines(&code);
        match running(&command, cx) {
            Some(run) => stop(run).into_any_element(),
            None => Button::new("run")
                .ghost()
                .small()
                .icon(IconName::Play)
                .tooltip("Run in the chat's folder")
                .on_click(move |_, window, cx| {
                    let command = command.clone();
                    window.dispatch_action(Box::new(RunCode { command }), cx)
                })
                .into_any_element(),
        }
    });
    div().flex().flex_row().gap_1().child(copy).children(run)
}

/// The turning ring of a run under way, and under the pointer the X that stops it.
fn stop(run: u64) -> Div {
    let group = SharedString::from("run-code");
    let ring = Button::new("running")
        .ghost()
        .small()
        .icon(Spinner::new().icon(IconName::LoaderCircle));
    let stop = Button::new("stop")
        .ghost()
        .small()
        .icon(IconName::Close)
        .tooltip("Stop")
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(StopCode { run }), cx));
    // Both are drawn and only fade: a part that leaves the layout under the pointer
    // would change between the frame's prepaint and its paint.
    div()
        .relative()
        .group(group.clone())
        .child(
            div()
                .group_hover(group.clone(), |this| this.opacity(0.))
                .child(ring),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .opacity(0.)
                .group_hover(group, |this| this.opacity(1.))
                .child(stop),
        )
}

/// The commands of a shell block: `$ ` prompts dropped, output lines of a console
/// transcript left out.
fn shell_lines(code: &str) -> String {
    let prompted = code.lines().any(|line| line.trim_start().starts_with("$ "));
    code.lines()
        .filter_map(|line| match prompted {
            true => line.trim_start().strip_prefix("$ "),
            false => Some(line),
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

impl Workspace {
    pub fn run_code(&mut self, action: &RunCode, _: &mut Window, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        if action.command.is_empty() {
            return;
        }
        let Some(chat) = self.chats.get_mut(&thread_id) else {
            return;
        };
        let command = action.command.clone();
        let run = chat.start_run(command.clone());
        let key = (thread_id.clone(), command.clone());
        cx.default_global::<Running>().runs.insert(key.clone(), run);
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().run_command(&asked, &command, run).await },
            move |ws, result, cx| {
                let runs = &mut cx.default_global::<Running>().runs;
                if runs.get(&key) == Some(&run) {
                    runs.remove(&key);
                }
                if let Some(chat) = ws.chats.get_mut(&thread_id) {
                    chat.finish_run(run, result.map_err(|error| format!("{error:#}")));
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    pub fn stop_code(&mut self, action: &StopCode, _: &mut Window, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let run = action.run;
        self.core.call(
            cx,
            move |client| async move { client.threads().stop_run(&thread_id, run).await },
            // It may have ended on its own meanwhile; its card says how.
            move |_, result, _| {
                if let Err(error) = result {
                    tracing::debug!("run {run} was not stopped: {error:#}");
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::shell_lines;

    #[test]
    fn a_console_transcript_keeps_only_its_commands() {
        assert_eq!(shell_lines("$ ls\nfile\n$ pwd\n/home"), "ls\npwd");
        assert_eq!(
            shell_lines("cargo build\ncargo test\n"),
            "cargo build\ncargo test"
        );
    }
}
