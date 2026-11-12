//! A project's chats, and opening one: a chat opened in this window stays open, so its
//! work goes on while another one is shown.

use std::path::PathBuf;

use dusk_protocol::{project::Project, thread::Thread};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::{
    pages::chat::transcript::Transcript,
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

pub fn render(
    ws: &Workspace,
    project: &Project,
    _: &mut Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let dir = project.dir.to_string_lossy();
    let threads: Vec<&Thread> = ws
        .threads
        .iter()
        .filter(|thread| thread.project.as_deref() == Some(dir.as_ref()))
        .filter(|thread| ws.matches_search(thread, cx))
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

/// One chat in a list; `source` adds where it was started, for the recent list.
pub fn chat_row(
    ws: &Workspace,
    thread: &Thread,
    source: Option<&str>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.primary);
    let id = thread.id.clone();
    let shown = ws.shown.as_deref() == Some(thread.id.as_str());
    let working = ws.chats.get(&thread.id).is_some_and(|chat| chat.working);
    let title = ws
        .chats
        .get(&thread.id)
        .and_then(|chat| chat.title.clone())
        .or_else(|| thread.title.clone())
        .unwrap_or_else(|| "New chat".to_owned());
    let prefix = if source.is_some() { "recent" } else { "chat" };
    let fading = ws.fading.contains(&thread.id);
    let dot = div().size_1p5().rounded_full().flex_none();
    // A chat whose agent works breathes; the others rest.
    let dot = if working {
        dot.bg(accent)
            .with_animation(
                SharedString::from(format!("{prefix}-working-{id}")),
                Animation::new(std::time::Duration::from_millis(1400)).repeat(),
                |dot, phase| {
                    let pulse = 0.35 + 0.65 * (phase * std::f32::consts::TAU).cos().abs();
                    dot.opacity(pulse)
                },
            )
            .into_any_element()
    } else {
        dot.bg(muted.opacity(0.4)).into_any_element()
    };
    let row = widgets::row(SharedString::from(format!("{prefix}-{id}")), shown, cx)
        .child(dot)
        .child(div().flex_1().min_w_0().truncate().child(title))
        .when_some(source.filter(|source| *source != "gui"), |row, source| {
            row.child(
                div()
                    .meta()
                    .text_color(muted.opacity(0.8))
                    .child(source.to_owned()),
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
        self.shown = None;
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
        let thread_id = thread_id.to_owned();
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                let history = threads.messages(&thread_id).await?;
                let thread_id = threads.resume(&thread_id, cwd).await?;
                let tasks = threads.tasks(&thread_id).await?;
                let access = threads.approvals(&thread_id).await?;
                anyhow::Ok((thread_id, history, tasks, access))
            },
            |ws, result, cx| match result {
                Ok((thread_id, history, tasks, access)) => {
                    let mut chat = Transcript::from_history(&history);
                    chat.tasks = tasks;
                    chat.access = access;
                    ws.chats.insert(thread_id.clone(), chat);
                    ws.show(thread_id, cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn show(&mut self, thread_id: String, cx: &mut Context<Self>) {
        self.shown = Some(thread_id);
        self.center = Center::Chat;
        self.context = None;
        self.jobs.clear();
        self.sources.clear();
        self.plan.text = None;
        self.plan.editing = false;
        self.refresh_inspector(cx);
        self.load_plan(cx);
        self.touched.clear();
        self.load_touched(cx);
        cx.notify();
    }
}
