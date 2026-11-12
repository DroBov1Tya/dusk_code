//! The message box: Enter sends, Shift+Enter starts a new line, Stop ends the turn.

use crate::ui::{Type, widgets::Focusable};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonRounded, ButtonVariants},
        input::{Paste, Textarea},
    },
    prelude::FluentBuilder,
    *,
};

use super::transcript::Transcript;
use crate::workspace::Workspace;

pub(super) const WIDTH: f32 = 780.;
/// The width of the columns beside the box.
const SIDE: f32 = crate::ui::size::BAR;
/// From the foot of the composer to the middle of the box's lower row of buttons: the
/// row under the box and the box's own padding.
const SIDE_LIFT: f32 = 40.;

/// A column beside the box, its controls centered and level with the box's buttons.
fn side() -> Div {
    div()
        .w(px(SIDE))
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pb(px(SIDE_LIFT))
}
/// The marks beside the box, the history on the left and the subagents and the context on
/// the right: one size and one shape, a circle, so the two sides mirror each other.
pub(super) const SIDE_MARK: f32 = crate::ui::size::CONTROL + 4.;
pub(super) const AGENTS_BUTTON: f32 = SIDE_MARK;

pub fn render(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    // Subagents may go on after the chat's own turn is over; Stop ends them too.
    let working = ws
        .shown_chat()
        .is_some_and(|chat| chat.working || chat.agents.values().any(|agent| agent.working));
    let button = if working {
        Button::new("stop")
            .danger()
            .rounded(ButtonRounded::Size(px(999.)))
            .icon(IconName::Square)
            .tooltip("Stop")
            .on_click(cx.listener(|ws, _, _, cx| ws.cancel(cx)))
    } else {
        Button::new("send")
            .primary()
            .rounded(ButtonRounded::Size(px(999.)))
            .icon(IconName::ArrowUp)
            .tooltip("Send")
            .on_click(cx.listener(|ws, _, window, cx| ws.send(window, cx)))
    };
    let attach = ws.attach_open.then(|| attach_menu(ws.shown.is_some(), cx));
    let queued = super::queue::render(ws, cx);
    // Output speed while the agent writes, from the core's rate events.
    let rate = ws
        .shown
        .as_ref()
        .filter(|_| working)
        .and_then(|id| ws.rates.get(id))
        .filter(|rate| **rate > 0)
        .map(|rate| {
            div()
                .meta()
                .text_color(cx.theme().muted_foreground)
                .pb_1()
                .child(format!("{rate} tok/s"))
        });
    let navigator =
        (ws.sent_rows.len() > 1).then(|| super::navigator::render(ws, &ws.sent_rows, window, cx));
    let mentions = mention_menu(ws, cx);
    let chips = strip(ws, cx);
    div()
        .relative()
        .px_4()
        .pt_2()
        .max_w(px(WIDTH + 2. * SIDE))
        .mx_auto()
        .w_full()
        .children(queued)
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                // The history on the left and the context on the right stand level with
                // the box's lower row of buttons, as wide as each other.
                .child(side().children(navigator))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            // One card: what goes with the message on top, the text under it
                            // across the whole width, and a row of controls at the bottom.
                            // The card grows upward, as the box sits at the foot of the chat.
                            div()
                                .flex()
                                .flex_col()
                                .rounded_2xl()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().muted)
                                .shadow_sm()
                                .relative()
                                .children(mentions.map(crate::ui::widgets::on_top))
                                .children(chips)
                                .child(
                                    div()
                                        .w_full()
                                        .px_2()
                                        .pt_1()
                                        .child(Textarea::new(&ws.composer).appearance(false)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .pb_2()
                                        .child(
                                            Button::new("attach")
                                                .ghost()
                                                .rounded(ButtonRounded::Size(px(999.)))
                                                .icon(IconName::Plus)
                                                .tooltip("Attach files or folders")
                                                .on_click(cx.listener(|ws, _, _, cx| {
                                                    ws.attach_open = !ws.attach_open;
                                                    cx.notify();
                                                })),
                                        )
                                        .children(super::model_bar::access(ws, cx))
                                        .child(div().flex_1())
                                        .children(rate)
                                        .child(mic_button(ws, cx))
                                        .child(button),
                                ),
                        )
                        // Under the box: the mode on the left, the model on the right.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .pt_1()
                                .px_1()
                                .child(super::model_bar::mode(ws, cx))
                                .child(super::model_bar::models(ws, window, cx)),
                        ),
                )
                // The subagents stand beside the box, not in it: they are not part of what
                // is sent.
                .child(
                    side()
                        .children(agents_button(ws, cx))
                        .child(super::model_bar::context(ws, cx)),
                ),
        )
        .children(attach.map(crate::ui::widgets::on_top))
        // Files copied in the file manager arrive as files, not as their names.
        .capture_action(cx.listener(|ws, _: &Paste, _, cx| {
            let Some(item) = cx.read_from_clipboard() else {
                return;
            };
            let mut found = false;
            for entry in item.entries() {
                match entry {
                    ClipboardEntry::ExternalPaths(paths) => {
                        ws.attachments.extend(paths.paths().iter().cloned());
                        found = true;
                    }
                    // A screenshot becomes a file; on send the core copies it among the
                    // chat's sources, as it does a dropped picture.
                    ClipboardEntry::Image(image)
                        if matches!(image.format, ImageFormat::Png | ImageFormat::Jpeg) =>
                    {
                        match ws.keep_pasted_image(image) {
                            Ok(path) => ws.attachments.push(path),
                            Err(error) => ws.fail(error, cx),
                        }
                        found = true;
                    }
                    _ => {}
                }
            }
            // A long text becomes a chip; the core keeps it as a file of the chat.
            if !found
                && let Some(text) = item.text()
                && text.chars().count() > PASTE_THRESHOLD
            {
                ws.pastes.push(text);
                found = true;
            }
            if found {
                cx.stop_propagation();
                cx.notify();
            }
        }))
        // Tab switches the mode, before the text box takes it as an indent.
        .capture_key_down(cx.listener(|ws, event: &KeyDownEvent, _, cx| {
            let keys = &event.keystroke;
            if keys.key == "tab" && !keys.modifiers.shift && !keys.modifiers.control {
                cx.stop_propagation();
                // Over a mention, Tab completes it, as in the TUI.
                match ws.mention_matches(cx).first().cloned() {
                    Some(name) => ws.complete_mention(name, cx),
                    None => ws.next_mode(cx),
                }
            }
        }))
}

/// The microphone: while it is on, what is said is added to the message.
fn mic_button(ws: &Workspace, cx: &mut Context<Workspace>) -> Button {
    let on = ws.dictating();
    Button::new("dictate")
        .rounded(ButtonRounded::Size(px(999.)))
        .icon(IconName::Mic)
        // The size of Send beside it; filled only while it listens.
        .when(on, |button| button.danger())
        .when(!on, |button| button.ghost())
        .tooltip(if on { "Stop dictating" } else { "Dictate" })
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_dictation(cx)))
}

const THUMB: f32 = 64.;

/// What goes with the message, in a row above the text that scrolls sideways: pictures as
/// thumbnails, other files and long pastes as chips, each with a button that drops it.
fn strip(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    if ws.attachments.is_empty() && ws.pastes.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let (muted, raised, border) = (theme.muted_foreground, theme.accent, theme.border);
    let mut row = div()
        .id("attachments")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .pt_3()
        .overflow_x_scroll();
    for (index, path) in ws.attachments.iter().enumerate() {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let remove = Button::new(("unattach", index))
            .ghost()
            .small()
            .icon(IconName::Close)
            .tooltip("Remove the attachment")
            .on_click(cx.listener(move |ws, _, _, cx| {
                if index < ws.attachments.len() {
                    ws.attachments.remove(index);
                }
                cx.notify();
            }));
        row = row.child(if is_picture(path) {
            div()
                .relative()
                .flex_none()
                .size(px(THUMB))
                .rounded_xl()
                .overflow_hidden()
                .border_1()
                .border_color(border)
                .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
                .child(div().absolute().top_0().right_0().child(remove))
        } else {
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(THUMB))
                .pl_3()
                .rounded_xl()
                .bg(raised)
                .meta()
                .child(crate::ui::widgets::icon(
                    if path.is_dir() {
                        IconName::Folder
                    } else {
                        IconName::File
                    },
                    muted,
                ))
                .child(name)
                .child(remove)
        });
    }
    for (index, text) in ws.pastes.iter().enumerate() {
        row = row.child(
            div()
                .flex()
                .flex_none()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(THUMB))
                .pl_3()
                .rounded_xl()
                .bg(raised)
                .meta()
                .child(crate::ui::widgets::icon(IconName::FileText, muted))
                .child(paste_label(text))
                .child(
                    Button::new(("unpaste", index))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Remove the pasted text")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if index < ws.pastes.len() {
                                ws.pastes.remove(index);
                            }
                            cx.notify();
                        })),
                ),
        );
    }
    Some(row)
}

fn is_picture(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
            )
        })
}

/// The chat's subagents at a glance, beside the box: the figure of the one that works (or
/// the latest), a count, and a ring in the signal color while any works. A click opens
/// the stage where each is seen at work.
fn agents_button(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = ws.shown_chat().filter(|chat| !chat.agents.is_empty())?;
    let thread_id = ws.shown.clone()?;
    let working = chat.agents.values().filter(|agent| agent.working).count();
    let lead = chat
        .agents
        .iter()
        .filter(|(_, agent)| agent.working)
        .min_by_key(|(_, agent)| agent.started)
        .or_else(|| chat.agents.iter().max_by_key(|(_, agent)| agent.started))?;
    let theme = cx.theme();
    let (signal, muted, border, surface) = (
        theme.warning,
        theme.muted_foreground,
        theme.border,
        theme.muted,
    );
    let color = if working > 0 { signal } else { muted };
    let hover = theme.list_hover;
    let total = chat.agents.len();
    let open = matches!(&ws.modal, Some(crate::modal::Modal::Agents { thread_id: open }) if open == &thread_id);
    let label = format!("Subagents, {working} active of {total}");
    let tile = div()
        .id("agents")
        .focusable_row(cx)
        .track_focus(&ws.agents_trigger_focus)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_expanded(open)
        .size(px(AGENTS_BUTTON))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(if working > 0 {
            signal.opacity(0.7)
        } else {
            border
        })
        .bg(surface)
        .cursor_pointer()
        .hover(move |button| button.bg(hover).border_color(muted.opacity(0.6)))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
        })
        .child(crate::columns::inspector::mascot::render_fox(
            "agents-lead",
            &lead.1.state,
            color,
            1.2,
        ))
        .on_click(cx.listener(move |ws, _, window, cx| {
            ws.modal = Some(crate::modal::Modal::Agents {
                thread_id: thread_id.clone(),
            });
            ws.agents_modal_focus.focus(window, cx);
            cx.notify();
        }));
    // The count sits on the circle's upper right, half outside it.
    const BADGE_ROOM: f32 = 6.;
    Some(
        div()
            .relative()
            .flex_none()
            .pt(px(BADGE_ROOM))
            .pr(px(BADGE_ROOM))
            // The room is outside the layout: the tile stays where it was, at its size.
            .mt(px(-BADGE_ROOM))
            .mr(px(-BADGE_ROOM))
            .child(tile)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .min_w(px(16.))
                    .h(px(16.))
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .meta()
                    .font_weight(FontWeight::MEDIUM)
                    .bg(if working > 0 { signal } else { surface })
                    .text_color(if working > 0 { theme.background } else { muted })
                    .border_1()
                    .border_color(border)
                    .child(chat.agents.len().to_string()),
            ),
    )
}

/// Pastes longer than this, in characters, become a chip, as in the terminal client.
const PASTE_THRESHOLD: usize = 1500;

/// `pasted text · 120 lines`.
pub fn paste_label(text: &str) -> String {
    format!(
        "pasted text \u{00B7} {}",
        crate::ui::widgets::count(text.lines().count(), "line")
    )
}

/// The profile and snippets an `@` word being typed may name; a click puts one in.
fn mention_menu(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let names = ws.mention_matches(cx);
    if names.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let mut menu = div()
        .absolute()
        .bottom(relative(1.))
        .left(px(48.))
        .w(px(240.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg();
    for (index, name) in names.into_iter().enumerate() {
        let label = format!("@{name}");
        menu = menu.child(
            crate::ui::widgets::row(("mention", index), index == 0, cx)
                .small()
                .child(label)
                .on_click(cx.listener(move |ws, _, _, cx| ws.complete_mention(name.clone(), cx))),
        );
    }
    Some(menu)
}

/// Files first, then folders; each opens the system's own picker.
fn attach_menu(has_chat: bool, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id("attach-menu")
        .absolute()
        .occlude()
        .bottom(px(64.))
        .left(px(24.))
        .w(px(220.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        .child(
            crate::ui::widgets::row("attach-files", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::File,
                    theme.muted_foreground,
                ))
                .child("Files")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(false, window, cx))),
        )
        .children(has_chat.then(|| {
            crate::ui::widgets::row("attach-plan", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::FileText,
                    theme.muted_foreground,
                ))
                .child("Plan")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.attach_open = false;
                    ws.edit_plan(cx);
                }))
        }))
        .child(
            crate::ui::widgets::row("attach-folders", false, cx)
                .child(crate::ui::widgets::icon(
                    IconName::Folder,
                    theme.muted_foreground,
                ))
                .child("Folders")
                .on_click(cx.listener(|ws, _, window, cx| ws.attach(true, window, cx))),
        )
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.attach_open = false;
            cx.notify();
        }))
}

impl Workspace {
    /// Writes a pasted picture next to the core's pictures, readable by this user only.
    fn keep_pasted_image(&self, image: &Image) -> anyhow::Result<std::path::PathBuf> {
        use anyhow::Context as _;
        let folder = self.status.artifacts_dir.join("pasted");
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(&folder)
            .context("cannot create the pasted pictures folder")?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let path = folder.join(format!("pasted-{stamp}.{}", image.format.extension()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options
            .open(&path)
            .context("cannot save the pasted picture")?;
        std::io::Write::write_all(&mut file, &image.bytes)
            .context("cannot save the pasted picture")?;
        Ok(path)
    }

    /// Where the `@` word before the cursor starts, and what follows its `@`.
    fn mention_word(&self, cx: &App) -> Option<(usize, String)> {
        let composer = self.composer.read(cx);
        let text = composer.value();
        let before = text.get(..composer.cursor())?;
        let start = before.rfind(char::is_whitespace).map_or(0, |at| {
            at + before[at..].chars().next().map_or(1, char::len_utf8)
        });
        let typed = before[start..].strip_prefix('@')?;
        typed
            .chars()
            .all(dusk_protocol::config::is_name_char)
            .then(|| (start, typed.to_owned()))
    }

    /// What the `@` word being typed may go on to, `me` first, at most eight.
    pub fn mention_matches(&self, cx: &App) -> Vec<String> {
        let Some((_, typed)) = self.mention_word(cx) else {
            return Vec::new();
        };
        self.me
            .names()
            .filter(|name| name.starts_with(typed.as_str()) && *name != typed)
            .take(8)
            .map(str::to_owned)
            .collect()
    }

    /// Puts `@name ` in place of the `@` word being typed.
    fn complete_mention(&mut self, name: String, cx: &mut Context<Self>) {
        let Some((start, _)) = self.mention_word(cx) else {
            return;
        };
        let Some(window) = cx.active_window() else {
            return;
        };
        let composer = self.composer.clone();
        let _ = window.update(cx, |_, window, cx| {
            composer.update(cx, |composer, cx| {
                let cursor = composer.cursor();
                composer.set_selected_range(start..cursor, cx);
                composer.replace(format!("@{name} "), window, cx);
            });
        });
        cx.notify();
    }

    /// Sends the composer's text to the shown chat, or to a new chat of the selected
    /// project when none is shown.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut text = self.composer.read(cx).value().trim().to_owned();
        for pasted in std::mem::take(&mut self.pastes) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&dusk_protocol::thread::wrap(&pasted));
        }
        // Attached files go as their paths; the core keeps them among the chat's sources.
        for path in std::mem::take(&mut self.attachments) {
            if !text.is_empty() {
                text.push('\n');
            }
            // A path with spaces goes quoted, so the core reads it as one.
            let path = path.display().to_string();
            match path.contains(char::is_whitespace) && !path.contains('"') {
                true => text.push_str(&format!("\"{path}\"")),
                false => text.push_str(&path),
            }
        }
        if text.is_empty() {
            return;
        }
        let shown = self.shown.clone();
        // A pending question takes the text as the user's own answer.
        let question = shown
            .as_ref()
            .and_then(|id| self.chats.get_mut(id))
            .and_then(|chat| chat.question.take());
        if let Some(question) = question {
            text = question.answer(&[], &text);
        } else if let Some(thread_id) = shown
            .clone()
            .filter(|id| self.chats.get(id).is_some_and(|chat| chat.working))
        {
            // The agent is busy: the text waits, editable, until the turn ends.
            self.composer
                .update(cx, |composer, cx| composer.set_value("", window, cx));
            self.queue(thread_id, text, window, cx);
            return;
        }
        // Without a project, a new chat works in the workspace root.
        let dir = Some(
            self.selected_project()
                .map_or_else(|| self.home.clone(), |project| project.dir.clone()),
        );
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                let (thread_id, new) = match (shown, dir) {
                    (Some(thread_id), _) => (thread_id, false),
                    (None, Some(dir)) => (threads.start(dir).await?, true),
                    (None, None) => anyhow::bail!("no project"),
                };
                threads.send(&thread_id, text, false).await?;
                anyhow::Ok((thread_id, new))
            },
            |ws, result, cx| match result {
                Ok((thread_id, new)) => {
                    if new {
                        ws.chats.insert(thread_id.clone(), Transcript::default());
                        ws.show(thread_id, cx);
                        ws.reload_threads(cx);
                    }
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Adds the picked files or folders to the next message.
    fn attach(&mut self, folders: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.attach_open = false;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |ws, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = ws.update(cx, |ws, cx| {
                ws.attachments.extend(paths);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let (turn, agents) = self.shown_chat().map_or((false, Vec::new()), |chat| {
            let agents = chat
                .agents
                .iter()
                .filter(|(_, agent)| agent.working)
                .map(|(id, _)| id.clone())
                .collect();
            (chat.working, agents)
        });
        let (chat_id, stopped) = (thread_id.clone(), agents.clone());
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                if turn {
                    threads.cancel(&thread_id).await?;
                }
                for agent in agents {
                    threads.stop_agent(&agent).await?;
                }
                anyhow::Ok(())
            },
            move |ws, result, cx| match result {
                Ok(()) => {
                    // A stopped agent may end without another event; the view must not wait.
                    if let Some(chat) = ws.chats.get_mut(&chat_id) {
                        for id in &stopped {
                            if let Some(agent) = chat.agents.get_mut(id) {
                                agent.stop();
                            }
                        }
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
