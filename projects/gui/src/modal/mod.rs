//! What opens over the window: the chat search, the project form and the file viewer.
//! One at a time; a click on the dimmed backdrop or the close button shuts it.

mod agent_chat;
mod agents;
mod arbiter;
mod chat_menu;
pub mod detail;
mod project;
mod search;
mod sphere;
mod viewer;
mod whats_new;

use std::path::PathBuf;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{InputState, TextareaState},
    },
    prelude::FluentBuilder,
    *,
};

use crate::workspace::Workspace;

pub use chat_menu::{FADE, render as chat_menu};
pub use viewer::ViewerContent;

pub enum Modal {
    Search,
    /// The settings' card that connects a provider, opened from the model menu.
    ConnectProvider,
    Project(ProjectForm),
    Viewer {
        path: PathBuf,
        content: ViewerContent,
    },
    Detail(detail::Detail),
    DeleteFile {
        path: PathBuf,
    },
    /// Every artifact a tab shows, deleted at once after a yes.
    DeleteFiles {
        paths: Vec<PathBuf>,
    },
    AgentChat {
        agent: String,
        name: String,
    },
    /// The arbiter of one chat; another chat shown meanwhile does not change it.
    Arbiter {
        thread_id: String,
    },
    /// The chat's subagents by role.
    Agents {
        thread_id: String,
    },
    Rename {
        thread_id: String,
        title: Entity<InputState>,
    },
    Delete {
        thread_id: String,
        title: String,
    },
    /// One background job of the shown chat: its whole command, how long it runs, what
    /// it printed, and a way to cancel it.
    Job {
        job_id: String,
        output: Option<String>,
    },
    /// What the update brought, once after it; `from` is the version left behind.
    WhatsNew {
        from: Option<String>,
    },
    /// A picture large over the window; a click outside closes it.
    Picture {
        path: PathBuf,
    },
    /// A speech button was pressed with no server connected for it.
    VoiceMissing {
        speech: crate::pages::connectors::voice::Speech,
    },
    /// Making or changing a scheduled task.
    Schedule(crate::pages::schedules::editor::Editor),
    /// Removing duskcode from the machine, in steps.
    Uninstall(crate::pages::settings::uninstall::Uninstall),
    /// Forgetting every connected provider, after a yes.
    RemoveProviders,
    /// Going back to a message of the chat erases it and all after it; asks first.
    Rollback {
        thread_id: String,
        sequence: u64,
        /// The item the view is cut at.
        at: usize,
        text: String,
    },
}

/// The form that adds a project, or changes the one with `id`.
pub struct ProjectForm {
    pub id: Option<String>,
    pub name: Entity<InputState>,
    pub description: Entity<TextareaState>,
    pub dir: Entity<InputState>,
    /// The names of the project's variables; their values stay in the core.
    pub env: Vec<String>,
    pub env_name: Entity<InputState>,
    pub env_value: Entity<InputState>,
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Option<Div> {
    let (title, width, body) = match ws.modal.as_ref()? {
        Modal::Search => ("Search chats".to_owned(), 640., search::render(ws, cx)),
        Modal::ConnectProvider => (
            "Providers".to_owned(),
            640.,
            div()
                .p_3()
                .child(crate::pages::settings::providers::connect_card(ws, cx)),
        ),
        Modal::Project(form) => (
            if form.id.is_some() {
                "Edit project"
            } else {
                "New project"
            }
            .to_owned(),
            640.,
            project::render(form, cx),
        ),
        Modal::Viewer { path, content } => (
            "File".to_owned(),
            880.,
            viewer::render(path, content, window, cx),
        ),
        Modal::Rename { title, .. } => {
            ("Rename chat".to_owned(), 480., chat_menu::rename(title, cx))
        }
        Modal::Delete { title, .. } => {
            ("Delete chat".to_owned(), 480., chat_menu::delete(title, cx))
        }
        Modal::Picture { path } => (
            "Picture".to_owned(),
            1100.,
            div().p_3().child(
                img(path.clone())
                    .w_full()
                    .max_h(px(760.))
                    .object_fit(ObjectFit::Contain),
            ),
        ),
        Modal::Uninstall(state) => (
            "Remove duskcode".to_owned(),
            520.,
            crate::pages::settings::uninstall::render(state, cx),
        ),
        Modal::Schedule(editor) => (
            if editor.id.is_some() {
                "Edit task"
            } else {
                "New task"
            }
            .to_owned(),
            600.,
            crate::pages::schedules::editor::render(ws, cx),
        ),
        Modal::WhatsNew { from } => (
            "What's new".to_owned(),
            640.,
            whats_new::render(from.as_deref(), cx),
        ),
        Modal::RemoveProviders => (
            "Remove all providers".to_owned(),
            480.,
            crate::pages::settings::providers::confirm_remove_all(cx),
        ),
        Modal::VoiceMissing { speech } => (
            "Voice".to_owned(),
            480.,
            crate::pages::connectors::voice::missing(*speech, cx),
        ),
        Modal::Rollback { .. } => ("Go back".to_owned(), 480., rollback(cx)),
        Modal::Arbiter { thread_id } => {
            let thread_id = thread_id.clone();
            (
                "Arbiter".to_owned(),
                1040.,
                arbiter::render(ws, &thread_id, cx),
            )
        }
        Modal::Agents { thread_id } => {
            let thread_id = thread_id.clone();
            let width = (window.viewport_size().width.as_f32() - 32.).clamp(220., 860.);
            (
                "Subagents".to_owned(),
                width,
                agents::render(ws, &thread_id, width >= 720., cx),
            )
        }
        Modal::Job { job_id, output } => (
            "Background job".to_owned(),
            720.,
            crate::columns::inspector::jobs::detail(ws, job_id, output.as_deref(), cx),
        ),
        Modal::AgentChat { agent, name } => {
            let (agent, name) = (agent.clone(), name.clone());
            let width = (window.viewport_size().width.as_f32() - 32.).clamp(220., 760.);
            (name, width, agent_chat::render(ws, &agent, cx))
        }
        Modal::DeleteFiles { paths } => (
            "Delete files".to_owned(),
            480.,
            chat_menu::delete_files(paths.len(), cx),
        ),
        Modal::DeleteFile { path } => (
            "Delete file".to_owned(),
            480.,
            chat_menu::delete_file(path, cx),
        ),
        Modal::Detail(item) => {
            let item = item.clone();
            (item.title().to_owned(), 720., detail::render(ws, &item, cx))
        }
    };
    // A subagent's chat opens from the list of subagents; Back returns to it.
    let back = matches!(ws.modal, Some(Modal::AgentChat { .. }))
        .then(|| ws.shown.clone())
        .flatten()
        .map(|thread_id| {
            Button::new("modal-back")
                .ghost()
                .small()
                .icon(IconName::ArrowLeft)
                .tooltip("Back to the subagents")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(Modal::Agents {
                        thread_id: thread_id.clone(),
                    });
                    cx.notify();
                }))
        });
    let agent_focus = matches!(
        ws.modal,
        Some(Modal::Agents { .. } | Modal::AgentChat { .. })
    )
    .then(|| ws.agents_modal_focus.clone());
    let theme = cx.theme();
    let panel = div()
        .id("modal-panel")
        // The window itself takes the keys, not its close button: a focused button
        // is drawn as if the pointer were on it.
        .when_some(agent_focus, |panel, focus| panel.track_focus(&focus))
        .w(px(width))
        .max_h(relative(0.86))
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg()
        // Clicks inside stay inside.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_5()
                .py_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .children(back)
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title)),
                )
                .child(
                    Button::new("modal-close")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Close")
                        .on_click(
                            cx.listener(|ws, _, window, cx| ws.close_modal_from_window(window, cx)),
                        ),
                ),
        )
        .child(
            div()
                .id("modal-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        );
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(72.))
            .bg(gpui_kit::black().opacity(0.45))
            // Nothing behind takes the mouse, scrolling included.
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|ws, _, window, cx| ws.close_modal_from_window(window, cx)),
            )
            .child(crate::ui::widgets::appear("modal-open", panel)),
    )
}

impl Workspace {
    pub fn close_modal_from_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.modal,
            Some(Modal::Agents { .. } | Modal::AgentChat { .. })
        ) {
            self.agents_trigger_focus.focus(window, cx);
        }
        self.close_modal(cx);
    }

    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        cx.notify();
    }
}

/// What going back costs, said before it happens: there is no way to bring it back.
fn rollback(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child("Go back to this message?")
        .child(div().text_sm().text_color(muted).child(
            "The message and everything after it are erased from the chat for good. \
             Files the agents wrote stay. The message returns to the box.",
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("rollback-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("rollback-go")
                        .danger()
                        .label("Go back")
                        .on_click(cx.listener(|ws, _, _, cx| ws.rollback(cx))),
                ),
        )
}
