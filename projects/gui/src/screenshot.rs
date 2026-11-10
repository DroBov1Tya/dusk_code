//! Development only (`--features screenshot`): with `DUSK_SHOTS` set, the window follows a
//! script and saves pictures of what it draws, rendered in memory, so no permission to
//! record the screen is needed. The script is steps separated by `;`:
//!
//! - `wait=MS`: let the core answer and the window draw;
//! - `project=N`, `thread=N`: choose the Nth project or chat of the lists;
//! - `center=chat|review|settings|connectors|artifacts`; `review`: open the review page;
//! - `settings=PAGE` (appearance, model, chat, safety, me, notifications, vcs, memory,
//!   advanced, updates, usage), `shelf=NAME` (connectors, skills, plugins, tools, agents),
//!   `artifacts`: open the page as a click would, with its data;
//! - `modal=search|project|none`, `right=all`: open a window; unfold the right column;
//! - `fake=approval|question`: put a request for approval, or a question, in the shown chat;
//! - `theme=NAME`: Dusk, Light, Dark, "Solar Dusk", Eclipse;
//! - `file=PATH[:LINE]`: open a file in the viewer;
//! - `shot=NAME`: save `DUSK_SHOT_DIR/NAME.png`; `quit`: end.

use std::{path::PathBuf, time::Duration};

use gpui_kit::*;

use crate::{
    connectors::Shelf,
    modal::Modal,
    settings::SettingsPage,
    workspace::{Center, Workspace},
};

pub fn run(opened: (AnyWindowHandle, Entity<Workspace>), cx: &mut App) {
    let (window, workspace) = opened;
    let Ok(script) = std::env::var("DUSK_SHOTS") else {
        return;
    };
    let dir = PathBuf::from(std::env::var("DUSK_SHOT_DIR").unwrap_or_else(|_| ".".into()));
    let _ = std::fs::create_dir_all(&dir);
    cx.spawn(async move |cx| {
        for step in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (name, value) = step.split_once('=').unwrap_or((step, ""));
            match name {
                "wait" => {
                    let ms = value.parse().unwrap_or(1000);
                    cx.background_executor()
                        .timer(Duration::from_millis(ms))
                        .await;
                }
                "shot" => {
                    let image = cx
                        .update_window(window, |_, window, cx| {
                            // The window may not be on screen, so it draws no frames of
                            // its own: draw the current state, then render that frame.
                            window.draw(cx).clear(cx);
                            window.render_to_image()
                        })
                        .ok()
                        .and_then(Result::ok);
                    match image {
                        Some(image) => {
                            let path = dir.join(format!("{value}.png"));
                            if let Err(error) = image.save(&path) {
                                eprintln!("cannot save {}: {error}", path.display());
                            }
                        }
                        None => eprintln!("cannot render {value}"),
                    }
                }
                "quit" => break,
                other => {
                    let (other, value) = (other.to_owned(), value.to_owned());
                    let workspace = workspace.clone();
                    let _ = cx.update_window(window, move |_, window, cx| {
                        workspace.update(cx, |ws, cx| act(ws, window, &other, &value, cx));
                    });
                }
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn act(
    ws: &mut Workspace,
    window: &mut Window,
    name: &str,
    value: &str,
    cx: &mut Context<Workspace>,
) {
    match name {
        "project" => {
            let id = value
                .parse::<usize>()
                .ok()
                .and_then(|n| ws.projects.get(n))
                .map(|project| project.id.clone());
            ws.select_project(id.as_deref(), cx);
        }
        "thread" => {
            let id = value
                .parse::<usize>()
                .ok()
                .and_then(|n| ws.threads.get(n))
                .map(|thread| thread.id.clone());
            if let Some(id) = id {
                ws.open_chat(&id, cx);
            }
        }
        "center" => {
            ws.center = match value {
                "review" => Center::Review,
                "settings" => Center::Settings,
                "connectors" => Center::Connectors,
                "artifacts" => Center::Artifacts,
                _ => Center::Chat,
            };
            cx.notify();
        }
        "review" => ws.open_review(cx),
        "settings" => {
            ws.settings_page = match value {
                "model" => SettingsPage::Model,
                "chat" => SettingsPage::Chat,
                "safety" => SettingsPage::Safety,
                "me" => SettingsPage::AboutMe,
                "notifications" => SettingsPage::Notifications,
                "vcs" => SettingsPage::VersionControl,
                "memory" => SettingsPage::Memory,
                "advanced" => SettingsPage::Advanced,
                "updates" => SettingsPage::Updates,
                "usage" => SettingsPage::Usage,
                _ => SettingsPage::Appearance,
            };
            ws.center = Center::Settings;
            ws.load_settings(cx);
        }
        "shelf" => {
            ws.shelf = match value {
                "skills" => Shelf::Skills,
                "plugins" => Shelf::Plugins,
                "tools" => Shelf::Tools,
                "agents" => Shelf::Agents,
                _ => Shelf::Connectors,
            };
            ws.center = Center::Connectors;
            ws.load_settings(cx);
        }
        "artifacts" => {
            ws.center = Center::Artifacts;
            ws.load_outputs(cx);
        }
        "modal" => match value {
            "search" => ws.modal = Some(Modal::Search),
            "project" => ws.open_form(None, window, cx),
            _ => ws.modal = None,
        },
        "right" => {
            ws.folded.clear();
            ws.right_hidden = false;
            ws.refresh_inspector(cx);
        }
        "fake" => {
            use dusk_protocol::{
                thread::ThreadEvent,
                tools::{ApprovalRequest, Question, Request},
            };
            let event = match value {
                "question" => ThreadEvent::Question(Question {
                    question: "Which cache should get() use for expired entries?".into(),
                    options: vec![
                        "Remove them when read (recommended)".into(),
                        "Keep them and mark as stale".into(),
                        "Evict on a timer".into(),
                    ],
                    multiple: false,
                }),
                _ => ThreadEvent::Approval(ApprovalRequest {
                    id: 1,
                    request: Request::Command {
                        command: "cargo test --workspace".into(),
                    },
                    session: Some("cargo".into()),
                }),
            };
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.apply(event);
            }
            cx.notify();
        }
        "theme" => crate::theme::change(cx, |prefs| prefs.theme = value.to_owned()),
        "file" => {
            let (path, line) = match value.rsplit_once(':') {
                Some((path, line)) if line.parse::<u32>().is_ok() => (path, line.parse().ok()),
                _ => (value, None),
            };
            ws.open_file(PathBuf::from(path), line, cx);
        }
        other => eprintln!("no such step: {other}"),
    }
}
