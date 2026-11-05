//! The projects, each folding over its chats, and the form that adds or changes one.

use std::path::PathBuf;

use dusk_protocol::project::Project;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::threads;
use crate::{
    widgets,
    workspace::{ProjectForm, Workspace},
};

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut list = div().flex().flex_col().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .pr_2()
            .child(widgets::heading("Projects", cx))
            .child(
                Button::new("add-project")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("Add a project")
                    .on_click(cx.listener(|ws, _, window, cx| ws.open_form(None, window, cx))),
            ),
    );
    if let Some(form) = &ws.form {
        list = list.child(render_form(form, cx));
    }
    if ws.projects.is_empty() && ws.form.is_none() {
        list = list.child(widgets::muted(
            "Add a project: a folder the agents work in. Its chats appear here.",
            cx,
        ));
    }
    for project in ws.projects.clone() {
        let open = !ws.collapsed.contains(&project.id);
        let active = ws.project.as_deref() == Some(project.id.as_str());
        let id = project.id.clone();
        let chevron = if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let header = widgets::row(SharedString::from(format!("project-{}", project.id)), active, cx)
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
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("New chat")
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |ws, _, window, cx| {
                            cx.stop_propagation();
                            ws.new_chat(&id, window, cx);
                        }
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("edit-project-{id}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings2)
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

fn render_form(form: &ProjectForm, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let editing = form.id.is_some();
    let mut actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            Button::new("project-cancel")
                .ghost()
                .small()
                .label("Cancel")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.form = None;
                    cx.notify();
                })),
        )
        .child(
            Button::new("project-save")
                .primary()
                .small()
                .label("Save")
                .on_click(cx.listener(|ws, _, _, cx| ws.save_form(cx))),
        );
    if let Some(id) = form.id.clone() {
        actions = actions.child(
            Button::new("project-remove")
                .danger()
                .small()
                .label("Remove")
                .tooltip("Removes the project; its chats and files stay")
                .on_click(cx.listener(move |ws, _, _, cx| ws.remove_project(&id, cx))),
        );
    }
    div()
        .mx_2()
        .mb_2()
        .p_2()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(if editing { "Edit project" } else { "New project" }),
        )
        .child(Input::new(&form.name))
        .child(Input::new(&form.description))
        .child(Input::new(&form.dir))
        .child(actions)
}

impl Workspace {
    pub fn reload_projects(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().list().await },
            |ws, result, cx| match result {
                Ok(projects) => {
                    if ws.project.is_none() {
                        ws.project = projects.first().map(|project| project.id.clone());
                        ws.refresh_inspector(cx);
                    }
                    ws.projects = projects;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn toggle_project(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.project.as_deref() != Some(id) {
            self.select_project(id, cx);
        } else if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        cx.notify();
    }

    pub fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.project.as_deref() == Some(id) {
            return;
        }
        self.project = Some(id.to_owned());
        self.collapsed.remove(id);
        self.files.clear();
        self.expanded.clear();
        self.git = None;
        self.refresh_inspector(cx);
    }

    fn open_form(&mut self, project: Option<Project>, window: &mut Window, cx: &mut Context<Self>) {
        let field = |placeholder: &'static str, value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder);
                state.set_value(value, window, cx);
                state
            })
        };
        let (id, name, description, dir) = match project {
            Some(project) => (
                Some(project.id),
                project.name,
                project.description,
                project.dir.display().to_string(),
            ),
            None => Default::default(),
        };
        self.form = Some(ProjectForm {
            id,
            name: field("Name", name, window, cx),
            description: field("Description", description, window, cx),
            dir: field("Folder, such as /Users/me/code/app", dir, window, cx),
        });
        cx.notify();
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.form else {
            return;
        };
        let id = form.id.clone();
        let name = form.name.read(cx).value().to_string();
        let description = form.description.read(cx).value().to_string();
        let dir = expand_home(form.dir.read(cx).value().trim());
        self.core.call(
            cx,
            move |client| async move { client.projects().save(id, name, description, dir).await },
            |ws, result, cx| match result {
                Ok(project) => {
                    ws.form = None;
                    ws.project = None;
                    ws.select_project(&project.id, cx);
                    ws.reload_projects(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn remove_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.projects().remove(&id).await },
            |ws, result, cx| match result {
                Ok(()) => {
                    ws.form = None;
                    ws.project = None;
                    ws.reload_projects(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}

/// `~/code` as the user types it; the core resolves the rest.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}
