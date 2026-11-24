//! Taking duskcode off the machine: a section on the Advanced page and a window of three
//! steps. The last one asks for a word typed. The removal is the core's, the one
//! `duskcode remove` runs, done from inside the app once its core has stopped; the app
//! then says so and closes itself.

use crate::ui::Type;
use std::time::Duration;

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        input::{Input, InputState},
    },
    *,
};

use super::line;
use crate::{modal::Modal, ui::widgets, workspace::Workspace};

/// What must be typed on the last step.
const WORD: &str = "REMOVE";

/// The window's state: the step, the choice about the chats' files, the typed word.
pub struct Uninstall {
    step: u8,
    files: bool,
    /// Where the removal is once the word was typed.
    phase: Phase,
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
            phase: Phase::Asking,
            typed,
            _typing,
        }
    }
}

enum Phase {
    Asking,
    Removing,
    /// Done; the keys the keychain kept, if any.
    Removed(Option<String>),
    Failed(String),
}

/// How long the last word stays before the app closes.
const FAREWELL: Duration = Duration::from_millis(2500);

/// The section at the end of the Advanced page.
pub fn section(cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pt_4()
        .child(widgets::caption("Remove duskcode", cx).px_0().pt_0())
        .child(super::list(
            [line(
                "Remove duskcode from this computer",
                "The programs, settings, API keys and chats. You are asked several times.",
                cx,
            )
            .child(
                Button::new("uninstall-open")
                    .danger()
                    .icon(IconName::Delete)
                    .label("Remove...")
                    .on_click(cx.listener(|ws, _, window, cx| {
                        ws.modal = Some(Modal::Uninstall(Uninstall::new(window, cx)));
                        cx.notify();
                    })),
            )],
            cx,
        ))
}

pub fn render(state: &Uninstall, cx: &mut Context<Workspace>) -> Div {
    match &state.phase {
        Phase::Asking => {}
        Phase::Removing => {
            return div()
                .p_5()
                .child(crate::ui::loading("Removing duskcode...", cx).px_0());
        }
        Phase::Removed(kept) => return removed(kept.as_deref(), cx),
        Phase::Failed(error) => return failed(error, cx),
    }
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
            "duskcode stops, removes itself in a few seconds and then closes. Type the word \
             below to go on.",
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
        .child(div().small().text_color(muted).child(text));
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
                    .small()
                    .text_color(danger)
                    .child("These files are your work. They will not come back either."),
            );
        }
    }
    if last {
        // What stays is said once more where the word is typed, so nobody expects it gone.
        let kept = if state.files {
            "The chats' files in ~/Documents/duskcode are deleted too."
        } else {
            "The chats' files in ~/Documents/duskcode are kept; cancel and tick the box on the second step to delete them too."
        };
        column = column
            .child(div().small().text_color(muted).child(kept))
            .child(Input::new(&state.typed));
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

/// The last word, shown while the app closes itself.
fn removed(kept: Option<&str>, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child("duskcode is removed"),
        )
        .children(kept.map(|kept| {
            div().small().text_color(cx.theme().danger).child(format!(
                "Some API keys stayed in the system keychain; delete them in the Keychain \
                     Access app. {kept}"
            ))
        }))
        .child(
            div()
                .small()
                .text_color(muted)
                .child("This window closes in a moment."),
        )
}

/// What stopped the removal. The core has stopped, so the app can only close.
fn failed(error: &str, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.danger)
                .child("The removal did not finish"),
        )
        .child(div().small().child(error.to_owned()))
        .child(div().small().text_color(theme.muted_foreground).child(
            "Close every other duskcode window and terminal, then open duskcode and remove it \
             again, or run `duskcode remove` in a terminal.",
        ))
        .child(
            div().flex().flex_row().justify_end().child(
                Button::new("uninstall-quit")
                    .label("Close duskcode")
                    .on_click(|_, _, cx| cx.quit()),
            ),
        )
}

impl Workspace {
    fn uninstall_next(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Uninstall(state)) = &mut self.modal else {
            return;
        };
        if !matches!(state.phase, Phase::Asking) {
            return;
        }
        if state.step < 3 {
            state.step += 1;
            cx.notify();
            return;
        }
        if state.typed.read(cx).value().trim() != WORD {
            return;
        }
        state.phase = Phase::Removing;
        let files = state.files;
        cx.notify();
        self.core
            .clone()
            .uninstall(cx, files, |ws, result, cx| ws.uninstall_done(result, cx));
    }

    /// Says how the removal went; after one that worked, the app closes itself.
    pub fn uninstall_done(
        &mut self,
        result: anyhow::Result<Option<String>>,
        cx: &mut Context<Self>,
    ) {
        let ok = result.is_ok();
        let phase = match result {
            Ok(kept) => Phase::Removed(kept),
            Err(error) => Phase::Failed(format!("{error:#}")),
        };
        match &mut self.modal {
            Some(Modal::Uninstall(state)) => state.phase = phase,
            // Closed meanwhile: what failed still has to be said.
            _ => {
                if let Phase::Failed(error) = phase {
                    self.fail(anyhow::anyhow!(error), cx);
                }
            }
        }
        cx.notify();
        if ok {
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(FAREWELL).await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    }

    /// Development only: the window at a step or a phase, without removing anything.
    #[cfg(feature = "screenshot")]
    pub fn dev_uninstall(&mut self, at: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut state = Uninstall::new(window, cx);
        match at {
            "2" => state.step = 2,
            "3" => {
                state.step = 3;
                state
                    .typed
                    .update(cx, |typed, cx| typed.set_value(WORD, window, cx));
            }
            "removing" => state.phase = Phase::Removing,
            _ => {}
        }
        self.modal = Some(Modal::Uninstall(state));
        match at {
            "removed" => self.uninstall_done(Ok(None), cx),
            "failed" => self.uninstall_done(
                Err(anyhow::anyhow!("another duskcode is still running")),
                cx,
            ),
            _ => cx.notify(),
        }
    }

    /// Whether the removal window may close: not while the core is being taken away.
    pub fn uninstall_busy(&self) -> bool {
        matches!(
            &self.modal,
            Some(Modal::Uninstall(Uninstall {
                phase: Phase::Removing | Phase::Removed(_),
                ..
            }))
        )
    }
}
