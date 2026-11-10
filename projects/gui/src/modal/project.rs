//! The project form: a name, a description to write at length, and a folder typed in
//! or picked with the system's own folder dialog.

use dusk_protocol::project::Project;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
    },
    *,
};

use super::{Modal, ProjectForm};
use crate::workspace::Workspace;

pub fn render(form: &ProjectForm, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let label = |text: &'static str| div().text_xs().text_color(muted).child(text);
    let mut actions = div().flex().flex_row().gap_2().justify_end().pt_2();
    if let Some(id) = form.id.clone() {
        actions = actions.child(
            Button::new("project-remove")
                .ghost()
                .label("Remove project")
                .tooltip("Its chats and files stay")
                .on_click(cx.listener(move |ws, _, _, cx| ws.remove_project(&id, cx))),
        );
        actions = actions.child(div().flex_1());
    }
    actions = actions
        .child(
            Button::new("project-cancel")
                .ghost()
                .label("Cancel")
                .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
        )
        .child(
            Button::new("project-save")
                .primary()
                .label("Save")
                .on_click(cx.listener(|ws, _, _, cx| ws.save_form(cx))),
        );
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .child(label("Name"))
        .child(Input::new(&form.name))
        .child(div().h_2())
        .child(label("Folder"))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(div().flex_1().child(Input::new(&form.dir)))
                .child(
                    Button::new("project-browse")
                        .icon(IconName::FolderOpen)
                        .label("Browse")
                        .on_click(cx.listener(|ws, _, window, cx| ws.browse_folder(window, cx))),
                ),
        )
        .child(div().h_2())
        .child(label(
            "Description: what the project is, for you and the agents",
        ))
        .child(Textarea::new(&form.description).h(px(120.)))
        .children(form.id.is_some().then(|| variables(form, cx)))
        .child(actions)
}

/// The names of the variables the project's commands run with, and a pair of boxes to
/// add one. A value is typed masked and never read back.
fn variables(form: &ProjectForm, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().gap_1().child(div().h_2()).child(
        div()
            .text_xs()
            .text_color(muted)
            .child("Variables: for the commands that run in this project; values are secret"),
    );
    for (index, name) in form.env.iter().enumerate() {
        let target = name.clone();
        column = column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .text_sm()
                .child(div().flex_1().child(name.clone()))
                .child(div().text_color(muted).child("••••••"))
                .child(
                    Button::new(("env-remove", index))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Remove the variable")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.remove_env(&target, cx))),
                ),
        );
    }
    column.child(
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(div().w(px(180.)).child(Input::new(&form.env_name)))
            .child(div().flex_1().child(Input::new(&form.env_value)))
            .child(
                Button::new("env-add")
                    .label("Set")
                    .on_click(cx.listener(|ws, _, window, cx| ws.set_env(window, cx))),
            ),
    )
}

impl Workspace {
    pub fn open_form(
        &mut self,
        project: Option<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, name, description, dir) = match project {
            Some(project) => (
                Some(project.id),
                project.name,
                project.description,
                project.dir.display().to_string(),
            ),
            None => Default::default(),
        };
        let field = |placeholder: &'static str,
                     value: String,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(placeholder);
                state.set_value(value, window, cx);
                state
            })
        };
        let description = cx.new(|cx| {
            let mut state = TextareaState::new(window, cx).placeholder("Optional");
            state.set_value(description, window, cx);
            state
        });
        let env_name = field("NAME", String::new(), window, cx);
        let env_value = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("value")
                .masked(true)
        });
        let known = id.clone();
        self.modal = Some(Modal::Project(ProjectForm {
            id,
            name: field("My app", name, window, cx),
            description,
            dir: field("/Users/me/code/app", dir, window, cx),
            env: Vec::new(),
            env_name,
            env_value,
        }));
        if let Some(id) = known {
            self.core.call(
                cx,
                move |client| async move { client.projects().env(&id).await },
                |ws, result, cx| match result {
                    Ok(names) => ws.show_env(names, cx),
                    Err(error) => ws.fail(error, cx),
                },
            );
        }
        cx.notify();
    }

    fn show_env(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        if let Some(Modal::Project(form)) = &mut self.modal {
            form.env = names;
        }
        cx.notify();
    }

    /// Sets the variable in the boxes, then empties them.
    fn set_env(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Project(form)) = &self.modal else {
            return;
        };
        let Some(id) = form.id.clone() else {
            return;
        };
        let name = form.env_name.read(cx).value().trim().to_owned();
        let value = form.env_value.read(cx).value().to_string();
        for field in [&form.env_name, &form.env_value] {
            field.update(cx, |field, cx| field.set_value("", window, cx));
        }
        self.core.call(
            cx,
            move |client| async move { client.projects().set_env(&id, &name, &value).await },
            |ws, result, cx| match result {
                Ok(names) => ws.show_env(names, cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn remove_env(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(Modal::Project(form)) = &self.modal else {
            return;
        };
        let (Some(id), name) = (form.id.clone(), name.to_owned()) else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move { client.projects().remove_env(&id, &name).await },
            |ws, result, cx| match result {
                Ok(names) => ws.show_env(names, cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn browse_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |ws, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = ws.update_in(cx, |ws, window, cx| {
                if let Some(Modal::Project(form)) = &ws.modal {
                    let shown = path.display().to_string();
                    let name_empty = form.name.read(cx).value().is_empty();
                    form.dir
                        .update(cx, |dir, cx| dir.set_value(shown, window, cx));
                    if name_empty && let Some(folder) = path.file_name() {
                        let folder = folder.to_string_lossy().into_owned();
                        form.name
                            .update(cx, |name, cx| name.set_value(folder, window, cx));
                    }
                }
            });
        })
        .detach();
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Project(form)) = &self.modal else {
            return;
        };
        let id = form.id.clone();
        let name = form.name.read(cx).value().to_string();
        let description = form.description.read(cx).value().to_string();
        let dir = dusk_core::app::expand_home(form.dir.read(cx).value().trim());
        self.core.call(
            cx,
            move |client| async move { client.projects().save(id, name, description, dir).await },
            |ws, result, cx| match result {
                Ok(project) => {
                    ws.modal = None;
                    ws.project = None;
                    ws.select_project(Some(&project.id), cx);
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
                    ws.modal = None;
                    ws.project = None;
                    ws.reload_projects(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
