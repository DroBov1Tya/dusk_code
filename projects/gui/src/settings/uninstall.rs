//! Taking duskcode off the machine: a section on the Advanced page and a window of three
//! steps. The last one asks for a word typed. The removal itself is the terminal client's
//! `remove`, started in the background; it waits for this window to close before it
//! deletes anything, because the window holds the data it deletes.

use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, ensure};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        input::{Input, InputState},
    },
    *,
};

use super::line;
use crate::{modal::Modal, widgets, workspace::Workspace};

/// What must be typed on the last step.
const WORD: &str = "REMOVE";

/// The window's state: the step, the choice about the chats' files, the typed word.
pub struct Uninstall {
    step: u8,
    files: bool,
    typed: Entity<InputState>,
    /// Redraws the window as the word is typed, so the button can open.
    _typing: Subscription,
}

impl Uninstall {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let typed = cx.new(|cx| InputState::new(window, cx).placeholder(WORD));
        let _typing = cx.observe(&typed, |_, _, cx| cx.notify());
        Self {
            step: 1,
            files: false,
            typed,
            _typing,
        }
    }
}

/// The section at the end of the Advanced page.
pub fn section(cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .pt_6()
        .child(widgets::caption("Remove duskcode", cx).px_0())
        .child(
            line(
                "Remove duskcode from this computer",
                "The programs, settings, API keys and chats. You are asked several times.",
                cx,
            )
            .child(
                Button::new("uninstall-open")
                    .small()
                    .danger()
                    .label("Remove...")
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.modal = Some(Modal::Uninstall(Uninstall::new(window, cx)));
                        cx.notify();
                    })),
            ),
        )
}

pub fn render(state: &Uninstall, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, danger) = (theme.muted_foreground, theme.danger);
    let (title, text, last) = match state.step {
        1 => (
            "Remove duskcode?",
            "This takes duskcode off this computer: the terminal and desktop programs, the app \
             icon, the settings, and every chat with its history.",
            false,
        ),
        2 => (
            "This cannot be undone",
            "Your API keys, chats, memories, skills and agents are erased for good, and there is \
             no trash to bring them back from. If you may want them, save a backup first with \
             /migrate in the terminal client.",
            false,
        ),
        _ => (
            "Last step",
            "duskcode will close now. The removal then runs in the background and finishes in a \
             few seconds. Type the word below to go on.",
            true,
        ),
    };
    let mut column = div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(danger)
                .child(title),
        )
        .child(div().text_sm().text_color(muted).child(text));
    if state.step == 2 {
        column = column.child(
            Checkbox::new("uninstall-files")
                .checked(state.files)
                .label("Also delete the files the chats made, in ~/Documents/duskcode")
                .on_click(cx.listener(|ws, checked: &bool, _, cx| {
                    if let Some(Modal::Uninstall(state)) = &mut ws.modal {
                        state.files = *checked;
                    }
                    cx.notify();
                })),
        );
        if state.files {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(danger)
                    .child("These files are your work. They will not come back either."),
            );
        }
    }
    if last {
        column = column.child(Input::new(&state.typed));
    }
    let ready = !last || state.typed.read(cx).value().trim() == WORD;
    column.child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .gap_2()
            .child(
                Button::new("uninstall-cancel")
                    .ghost()
                    .label("Cancel")
                    .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
            )
            .child(
                Button::new("uninstall-next")
                    .danger()
                    .disabled(!ready)
                    .label(if last { "Remove and quit" } else { "Continue" })
                    .on_click(cx.listener(|ws, _, _, cx| ws.uninstall_next(cx))),
            ),
    )
}

impl Workspace {
    fn uninstall_next(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Uninstall(state)) = &mut self.modal else {
            return;
        };
        if state.step < 3 {
            state.step += 1;
            cx.notify();
            return;
        }
        if state.typed.read(cx).value().trim() != WORD {
            return;
        }
        match start_removal(state.files) {
            Ok(()) => cx.quit(),
            Err(error) => {
                self.modal = None;
                self.fail(error, cx);
            }
        }
    }
}

/// Starts `duskcode remove --yes` beside this program, detached, so it outlives it.
fn start_removal(files: bool) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find this program")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let cli = exe
        .parent()
        .map(|folder| folder.join("duskcode"))
        .context("this program has no folder")?;
    ensure!(
        cli.is_file(),
        "the terminal client is not beside this program ({}); run `duskcode remove` in a terminal",
        cli.display()
    );
    let mut command = Command::new(&cli);
    command.args(["remove", "--yes"]);
    if files {
        command.arg("--files");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group, so closing this window does not take it along.
        command.process_group(0);
    }
    command.spawn().context("cannot start the removal")?;
    Ok(())
}
