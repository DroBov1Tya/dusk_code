//! Buttons in the corner of a code block: Copy for every block, Run for shell code. Run
//! starts at once; the core runs the code in the chat's folder, confined as the chat's
//! tools are, and the output lands in the chat under a bar that fills while it runs.

use std::hash::{DefaultHasher, Hash, Hasher};

use gpui_kit::base::text::CodeBlock;
use gpui_kit::{
    component::{
        IconName, Sizable,
        button::{Button, ButtonVariants},
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
        Button::new("run")
            .ghost()
            .small()
            .icon(IconName::Play)
            .tooltip("Run in the chat's folder")
            .on_click(move |_, window, cx| {
                window.dispatch_action(
                    Box::new(RunCode {
                        command: shell_lines(&code),
                    }),
                    cx,
                )
            })
    });
    div().flex().flex_row().gap_1().child(copy).children(run)
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
        let asked = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().run_command(&asked, &command).await },
            move |ws, result, cx| {
                if let Some(chat) = ws.chats.get_mut(&thread_id) {
                    chat.finish_run(run, result.map_err(|error| format!("{error:#}")));
                }
                cx.notify();
            },
        );
        cx.notify();
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
