//! The projects, each folding over its chats. A project row is quiet until the pointer
//! is over it; its chats sit indented below with their own, lighter hover.

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::threads;
use crate::{ui::widgets, workspace::Workspace};

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.accent);
    let mut list = div().flex().flex_col().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .pr_3()
            .child(widgets::caption("Projects", cx))
            .child(
                Button::new("add-project")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip("Add a project")
                    .on_click(cx.listener(|ws, _, window, cx| ws.open_form(None, window, cx))),
            ),
    );
    if ws.projects.is_empty() {
        list = list.child(widgets::muted(
            "A project is a folder the agents work in. Its chats gather under it.",
            cx,
        ));
    }
    for project in ws.projects.clone() {
        let open = !ws.collapsed.contains(&project.id);
        let id = project.id.clone();
        let chevron = if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let header = div()
            .id(SharedString::from(format!("project-{}", project.id)))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .mx_2()
            .mt_2()
            .px_2()
            .h(px(32.))
            .rounded_md()
            .cursor_pointer()
            .hover(move |row| row.bg(hover))
            .child(widgets::icon(chevron, muted))
            .child(widgets::icon(IconName::Folder, muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(project.name.clone()),
            )
            .child(
                Button::new(SharedString::from(format!("new-chat-{id}")))
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip("New chat in this project")
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |ws, _, window, cx| {
                            cx.stop_propagation();
                            ws.new_chat(Some(&id), window, cx);
                        }
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("edit-project-{id}")))
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .tooltip("Edit the project")
                    .on_click(cx.listener({
                        let project = project.clone();
                        move |ws, _, window, cx| {
                            cx.stop_propagation();
                            ws.open_form(Some(project.clone()), window, cx);
                        }
                    })),
            )
            .on_click(cx.listener(move |ws, _, _, cx| ws.toggle_project(&id, cx)));
        list = list.child(header);
        if open {
            list = list.child(threads::render(ws, &project, window, cx));
        }
    }
    list
}

impl Workspace {
    pub fn reload_projects(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().list().await },
            |ws, result, cx| match result {
                Ok(projects) => {
                    ws.projects = projects;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn toggle_project(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        cx.notify();
    }

    /// The project the right column and new chats use; `None` is the workspace root.
    pub fn select_project(&mut self, id: Option<&str>, cx: &mut Context<Self>) {
        if self.project.as_deref() == id {
            return;
        }
        self.project = id.map(str::to_owned);
        self.files.clear();
        self.expanded.clear();
        self.git = None;
        self.refresh_inspector(cx);
    }
}
