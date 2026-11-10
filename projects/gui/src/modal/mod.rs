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

use std::path::PathBuf;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{InputState, TextareaState},
    },
    *,
};

use crate::workspace::Workspace;

pub use chat_menu::{FADE, render as chat_menu};
pub use viewer::ViewerContent;

pub enum Modal {
    Search,
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
    /// A picture large over the window; a click outside closes it.
    Picture {
        path: PathBuf,
    },
    /// A shell block from an answer, to run once the user says yes.
    RunCode {
        thread_id: String,
        command: String,
    },
}

/// The form that adds a project, or changes the one with `id`.
pub struct ProjectForm {
    pub id: Option<String>,
    pub name: Entity<InputState>,
    pub description: Entity<TextareaState>,
    pub dir: Entity<InputState>,
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Option<Div> {
    let (title, width, body) = match ws.modal.as_ref()? {
        Modal::Search => ("Search chats".to_owned(), 640., search::render(ws, cx)),
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
        Modal::RunCode { command, .. } => (
            "Run command".to_owned(),
            640.,
            run_code(&command.clone(), cx),
        ),
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
            (
                "Subagents".to_owned(),
                560.,
                agents::render(ws, &thread_id, cx),
            )
        }
        Modal::AgentChat { agent, name } => {
            let (agent, name) = (agent.clone(), name.clone());
            (name, 760., agent_chat::render(ws, &agent, cx))
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
    let theme = cx.theme();
    let panel = div()
        .id("modal-panel")
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
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    Button::new("modal-close")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
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
                cx.listener(|ws, _, _, cx| ws.close_modal(cx)),
            )
            .child(panel),
    )
}

impl Workspace {
    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        cx.notify();
    }
}

/// The command in full before it runs: it came from the model.
fn run_code(command: &str, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, surface) = (theme.muted_foreground, theme.muted);
    let mono = crate::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child("The model wrote this. It runs in the chat's folder, confined as the chat's tools are, and the output goes to the message box."),
        )
        .child(
            div()
                .id("run-code-text")
                .max_h(px(280.))
                .overflow_y_scroll()
                .p_3()
                .rounded_md()
                .bg(surface)
                .text_xs()
                .font_family(mono)
                .child(command.to_owned()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("run-code-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("run-code-run")
                        .primary()
                        .label("Run")
                        .on_click(cx.listener(|ws, _, _, cx| ws.run_code(cx))),
                ),
        )
}
