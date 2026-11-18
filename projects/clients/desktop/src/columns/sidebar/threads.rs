//! A project's chats, and opening one: a chat opened in this window stays open, so its
//! work goes on while another one is shown.

use std::path::PathBuf;

use dusk_protocol::{project::Project, thread::Thread};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::chat::transcript::Transcript,
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

/// The box the working ring and the resting dot stand in.
const MARK: f32 = 12.;

pub fn render(
    ws: &Workspace,
    project: &Project,
    _: &mut Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let dir = project.dir.to_string_lossy();
    let threads: Vec<&Thread> = super::listed(ws, cx)
        .into_iter()
        .filter(|thread| thread.project.as_deref() == Some(dir.as_ref()))
        .collect();
    let mut list = div().flex().flex_col().gap_0p5().pl_5().pt_1();
    if threads.is_empty() {
        return list.child(widgets::muted("No chats yet.", cx));
    }
    for thread in threads {
        list = list.child(chat_row(ws, thread, None, cx));
    }
    list
}

/// The chats that stand under no saved project: started outside one, or in a folder that
/// is not a project any more.
pub fn loose(ws: &Workspace, cx: &App) -> Vec<Thread> {
    super::listed(ws, cx)
        .into_iter()
        .filter(|thread| project_of(ws, thread).is_none())
        .cloned()
        .collect()
}

fn project_of<'a>(ws: &'a Workspace, thread: &Thread) -> Option<&'a Project> {
    let dir = thread.project.as_deref()?;
    ws.projects
        .iter()
        .find(|project| project.dir.to_string_lossy() == dir)
}

/// Where a chat belongs, for a list that is not grouped: where it was started when that
/// is not this window, or else the name of its project.
pub fn place(ws: &Workspace, thread: &Thread) -> Option<String> {
    if thread.source != "gui" {
        return Some(thread.source.clone());
    }
    project_of(ws, thread).map(|project| project.name.clone())
}

/// One chat in a list; `place` is said at the row's end, in a list that is not grouped.
pub fn chat_row(
    ws: &Workspace,
    thread: &Thread,
    place: Option<String>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.primary);
    let id = thread.id.clone();
    let shown = ws.shown.as_deref() == Some(thread.id.as_str());
    let working = ws
        .chats
        .get(&thread.id)
        .is_some_and(|chat| chat.working || chat.agents_working);
    let title = ws
        .chats
        .get(&thread.id)
        .and_then(|chat| chat.title.clone())
        .or_else(|| thread.title.clone())
        .unwrap_or_else(|| "New chat".to_owned());
    let prefix = "chat";
    let fading = ws.fading.contains(&thread.id);
    // A chat whose agent works turns a small ring; the others rest on a dot. Both stand
    // in a box of one size, so the titles keep one left edge.
    let mark = div()
        .size(px(MARK))
        .flex_none()
        .flex()
        .items_center()
        .justify_center();
    let dot = if working {
        mark.child(
            Icon::new(IconName::LoaderCircle)
                .size(px(MARK))
                .text_color(accent)
                .with_animation(
                    SharedString::from(format!("{prefix}-working-{id}")),
                    Animation::new(std::time::Duration::from_millis(900)).repeat(),
                    |icon, delta| icon.rotate(percentage(delta)),
                ),
        )
    } else {
        mark.child(div().size_1p5().rounded_full().bg(muted.opacity(0.4)))
    };
    let row = widgets::row(SharedString::from(format!("{prefix}-{id}")), shown, cx)
        .child(dot)
        .when(thread.temporary, |row| {
            row.child(
                Icon::default()
                    .path(crate::ui::icons::TEMPORARY)
                    .size_3()
                    .text_color(muted),
            )
        })
        .child(div().flex_1().min_w_0().truncate().child(title))
        .children(pulls_badge(ws, thread, prefix, cx))
        .when_some(place, |row, place| {
            row.child(
                div()
                    .flex_none()
                    .max_w(px(96.))
                    .truncate()
                    .meta()
                    .text_color(muted.opacity(0.8))
                    .child(place),
            )
        })
        .on_click({
            let id = id.clone();
            cx.listener(move |ws, _, _, cx| ws.open_chat(&id, cx))
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, event: &MouseDownEvent, _, cx| {
                ws.chat_menu = Some((id.clone(), event.position));
                cx.notify();
            }),
        );
    if !fading {
        return row.into_any_element();
    }
    row.with_animation(
        SharedString::from(format!("{prefix}-fade-{}", thread.id)),
        Animation::new(crate::modal::FADE).with_easing(ease_out_quint()),
        |row, delta| row.opacity(1. - delta),
    )
    .into_any_element()
}

/// A pull request icon and count when the chat's project branch has pull requests,
/// tinted by their worst checks; its tooltip names them.
fn pulls_badge(ws: &Workspace, thread: &Thread, prefix: &str, cx: &App) -> Option<Stateful<Div>> {
    use crate::columns::inspector::pulls::{check_color, state_name};
    let dir = thread.project.as_deref()?;
    let project = ws
        .projects
        .iter()
        .find(|project| project.dir.to_string_lossy() == dir)?;
    let summary = ws.pulls.get(&project.id)?;
    let muted = cx.theme().muted_foreground;
    let color = summary.checks.map_or(muted, |state| check_color(state, cx));
    let said = summary
        .pulls
        .iter()
        .map(|pull| {
            format!(
                "#{} {} ({})",
                pull.number,
                pull.title,
                state_name(pull.state)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(
        div()
            .id(SharedString::from(format!("{prefix}-pulls-{}", thread.id)))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_0p5()
            .child(
                Icon::default()
                    .path(crate::ui::icons::GIT_PULL)
                    .size_3()
                    .text_color(color),
            )
            .child(
                div()
                    .meta()
                    .text_color(muted)
                    .child(summary.pulls.len().to_string()),
            )
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(said.clone()).build(window, cx)
            }),
    )
}

impl Workspace {
    pub fn matches_search(&self, thread: &Thread, cx: &App) -> bool {
        let query = self.search.read(cx).value().trim().to_lowercase();
        query.is_empty()
            || thread
                .title
                .as_deref()
                .is_some_and(|title| title.to_lowercase().contains(&query))
    }

    /// A new chat in a project's folder, or in the workspace root without one. Only its
    /// window opens: the thread is made when the first message is sent.
    pub fn new_chat(
        &mut self,
        project_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_project(project_id, cx);
        self.drafts.new_chat();
        self.shown = None;
        self.temporary_next = false;
        self.center = Center::Chat;
        self.context = None;
        self.jobs.clear();
        self.sources.clear();
        self.plan.text = None;
        self.plan.editing = false;
        self.touched.clear();
        self.refresh_inspector(cx);
        self.composer
            .update(cx, |composer, cx| composer.focus(window, cx));
        cx.notify();
    }

    /// A new chat that is never written to disk; like any new chat it is made when the
    /// first message is sent.
    pub fn new_temporary_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.selected_project().map(|project| project.id.clone());
        self.new_chat(project.as_deref(), window, cx);
        self.temporary_next = true;
        cx.notify();
    }

    /// Whether the shown chat, or the one about to start, is a temporary one.
    pub fn temporary_shown(&self) -> bool {
        match &self.shown {
            Some(shown) => self
                .threads
                .iter()
                .any(|thread| thread.id == *shown && thread.temporary),
            None => self.temporary_next,
        }
    }

    pub fn open_chat(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        let Some(thread) = self
            .threads
            .iter()
            .find(|thread| thread.id == thread_id)
            .cloned()
        else {
            return;
        };
        let project = thread.project.as_deref().and_then(|dir| {
            self.projects
                .iter()
                .find(|p| p.dir.to_string_lossy() == dir)
                .map(|p| p.id.clone())
        });
        self.select_project(project.as_deref(), cx);
        if self.chats.contains_key(thread_id) {
            self.show(thread_id.to_owned(), cx);
            return;
        }
        // A chat from before folders were recorded opens in the workspace root.
        let cwd = thread
            .project
            .clone()
            .map_or_else(|| self.home.clone(), PathBuf::from);
        let id = thread_id.to_owned();
        let asked = id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().messages(&asked).await },
            move |ws, result, cx| match result {
                Ok(history) => {
                    // The chat is in place before the thread is resumed, so every
                    // event from then on lands in it: the answer that opens the
                    // thread and the turn another client already runs.
                    ws.chats
                        .insert(id.clone(), Transcript::from_history(&history));
                    let open = id.clone();
                    ws.core.call(
                        cx,
                        move |client| async move {
                            let threads = client.threads();
                            let thread_id = threads.resume(&open, cwd).await?;
                            let tasks = threads.tasks(&thread_id).await?;
                            let access = threads.approvals(&thread_id).await?;
                            let pins = threads.pins(&thread_id).await?;
                            anyhow::Ok((thread_id, tasks, access, pins))
                        },
                        move |ws, result, cx| match result {
                            Ok((thread_id, tasks, access, pins)) => {
                                if let Some(chat) = ws.chats.get_mut(&thread_id) {
                                    chat.tasks = tasks;
                                    chat.access = access;
                                    chat.pins = pins;
                                }
                                ws.show(thread_id, cx);
                            }
                            Err(error) => {
                                ws.chats.remove(&id);
                                ws.fail(error, cx);
                            }
                        },
                    );
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn show(&mut self, thread_id: String, cx: &mut Context<Self>) {
        self.history.moved();
        self.shown = Some(thread_id);
        self.center = Center::Chat;
        self.context = None;
        self.jobs.clear();
        self.sources.clear();
        self.plan.text = None;
        self.plan.editing = false;
        self.refresh_inspector(cx);
        self.load_plan(cx);
        self.load_plan_counts(cx);
        self.touched.clear();
        self.load_touched(cx);
        cx.notify();
    }
}
