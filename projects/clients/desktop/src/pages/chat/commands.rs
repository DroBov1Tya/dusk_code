//! Slash commands in the message box: the list offered while a `/word` is typed, and
//! running one. The core says what a command is and does the work; a command that only
//! shows something opens the window's own place for it.

use dusk_protocol::client::{Answer, COMMANDS, Command, parse};
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    modal::Modal,
    pages::settings::SettingsPage,
    ui::Type,
    workspace::{Center, Picker, Right, Workspace},
};

/// The commands this window has a place for; the rest are the TUI's panels so far.
const OFFERED: &[&str] = &[
    "/new",
    "/compact",
    "/fork",
    "/goal",
    "/resume",
    "/model",
    "/effort",
    "/context",
    "/settings",
    "/update",
    "/migrate",
];

/// The commands the `/word` being typed may go on to; a click puts one in.
pub(super) fn menu(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let matches = ws.command_matches(cx);
    if matches.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mut menu = div()
        .absolute()
        .bottom(relative(1.))
        .left(px(48.))
        .w(px(420.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg();
    for (index, (name, help)) in matches.into_iter().enumerate() {
        menu = menu.child(
            crate::ui::widgets::row(("command", index), index == 0, cx)
                .small()
                .child(div().flex_none().w(px(84.)).child(name))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(help),
                )
                .on_click(
                    cx.listener(move |ws, _, window, cx| ws.complete_command(name, window, cx)),
                ),
        );
    }
    Some(menu)
}

impl Workspace {
    /// What the box's text may go on to while it is one `/word`.
    pub(super) fn command_matches(&self, cx: &App) -> Vec<(&'static str, &'static str)> {
        let typed = self.composer.read(cx).value();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            return Vec::new();
        }
        // In the order this window offers them: the common ones first.
        OFFERED
            .iter()
            .filter(|name| name.starts_with(typed.as_ref()))
            .filter_map(|name| COMMANDS.iter().find(|(known, _)| known == name))
            .copied()
            .collect()
    }

    pub(super) fn complete_command(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = format!("{name} ");
        self.composer
            .update(cx, |composer, cx| composer.set_value(text, window, cx));
        cx.notify();
    }

    /// Runs the text when it is a command that does not go to the thread as a message.
    /// True when the text was taken: run, or refused with the reason shown.
    pub(super) fn run_command(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let command = match parse(text) {
            None => return false,
            Some(Err(error)) => {
                self.fail(error, cx);
                return true;
            }
            Some(Ok(command)) => command,
        };
        if command.runs_in_thread() {
            // The thread runs it; without a chat there is nothing to run it on.
            if self.shown.is_some() {
                return false;
            }
            self.fail(anyhow::anyhow!("open a chat to use this command"), cx);
            return true;
        }
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        match command {
            Command::New => self.new_chat(None, window, cx),
            Command::Temporary => self.new_temporary_chat(window, cx),
            Command::Resume { .. } => self.modal = Some(Modal::Search),
            Command::Model { name: Some(name) } => self.choose_model(name, cx),
            Command::Model { name: None } => self.open_picker(Picker::Model, cx),
            Command::Effort { level: Some(level) } => self.choose_effort(&level, cx),
            Command::Effort { level: None } => self.open_picker(Picker::Effort, cx),
            Command::Settings => {
                self.center = Center::Settings;
                self.load_settings(cx);
            }
            Command::Update { .. } => {
                self.center = Center::Settings;
                self.settings_page = SettingsPage::Updates;
                self.load_settings(cx);
                self.load_page(SettingsPage::Updates, cx);
            }
            Command::Context { .. } => {
                self.right_hidden = false;
                self.folded.remove(&Right::Statistics);
                self.load_context(cx);
            }
            Command::Migrate { .. } => self.core.call(
                cx,
                move |client| async move { client.command(command).await },
                |ws, result, cx| {
                    match result {
                        Ok(Answer::Migrated(path)) => {
                            ws.notice = Some(format!("Saved to {}", path.display()));
                        }
                        Ok(_) => {}
                        Err(error) => return ws.fail(error, cx),
                    }
                    cx.notify();
                },
            ),
            other => {
                let name = text.split_whitespace().next().unwrap_or_default();
                tracing::debug!("no place in the window for {other:?}");
                self.fail(anyhow::anyhow!("{name} has no window here yet"), cx);
            }
        }
        cx.notify();
        true
    }

    /// Opens a menu of the model bar, whatever was open.
    fn open_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        self.picker = None;
        self.picker_closed = None;
        self.toggle_picker(picker, cx);
    }
}
