//! What opens over the window: the chat search, the project form and the file viewer.
//! One at a time; a click on the dimmed backdrop or the close button shuts it.

mod chat_menu;
pub mod detail;
mod project;
mod search;
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

pub use chat_menu::render as chat_menu;
pub use viewer::ViewerContent;

pub enum Modal {
    Search,
    Project(ProjectForm),
    Viewer { path: PathBuf, content: ViewerContent },
    Detail(detail::Detail),
    Rename { thread_id: String, title: Entity<InputState> },
    Delete { thread_id: String, title: String },
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
            if form.id.is_some() { "Edit project" } else { "New project" }.to_owned(),
            640.,
            project::render(form, cx),
        ),
        Modal::Viewer { path, content } => (
            "File".to_owned(),
            880.,
            viewer::render(path, content, window, cx),
        ),
        Modal::Rename { title, .. } => ("Rename chat".to_owned(), 480., chat_menu::rename(title, cx)),
        Modal::Delete { title, .. } => ("Delete chat".to_owned(), 480., chat_menu::delete(title, cx)),
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
        .child(div().id("modal-body").flex_1().min_h_0().overflow_y_scroll().child(body));
    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(72.))
            .bg(gpui_kit::black().opacity(0.45))
            .on_mouse_down(MouseButton::Left, cx.listener(|ws, _, _, cx| ws.close_modal(cx)))
            .child(panel),
    )
}

impl Workspace {
    pub fn close_modal(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        cx.notify();
    }
}
