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
//! - `modal=search|project|agents|none`, `right=all`: open a window; unfold the right column;
//! - `detail=mcp|skill`: open the window of the first of those;
//! - `fake=agents`: six subagents of different roles at different points of their work;
//! - `fake=approval|question`: put a request for approval, or a question, in the shown chat;
//! - `picker=model|mode|access|effort|more`, `attach`, `menu=chat|path|delete|rename|run`:
//!   open a menu or window of the chat;
//! - `theme=NAME`: Dusk, Light, Dark, "Solar Dusk", Eclipse;
//! - `file=PATH[:LINE]`: open a file in the viewer;
//! - `shot=NAME`: save `DUSK_SHOT_DIR/NAME.png`; `quit`: end.

use std::{path::PathBuf, time::Duration};

use gpui_kit::{component::input::InputState, *};

use crate::{
    connectors::Shelf,
    modal::Modal,
    settings::SettingsPage,
    workspace::{Center, Picker, Workspace},
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
        "detail" => {
            use crate::modal::detail::Detail;
            let first = |names: Vec<String>| names.into_iter().next();
            let detail = ws.settings.as_ref().and_then(|settings| match value {
                "skill" => first(settings.skills.iter().map(|s| s.name.clone()).collect())
                    .map(|name| Detail::Skill { name, file: None }),
                _ => first(settings.mcp.iter().map(|m| m.name.clone()).collect()).map(Detail::Mcp),
            });
            if let Some(detail) = detail {
                ws.open_detail(detail, cx);
            }
        }
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
            "agents" => {
                ws.modal = ws
                    .shown
                    .clone()
                    .map(|thread_id| Modal::Agents { thread_id });
            }
            _ => ws.modal = None,
        },
        "picker" => {
            if ws.providers.is_empty() {
                ws.load_models(cx);
            }
            ws.picker = Some(match value {
                "mode" => Picker::Mode,
                "access" => Picker::Access,
                "effort" => Picker::Effort,
                "more" => Picker::MoreModels,
                _ => Picker::Model,
            });
            cx.notify();
        }
        "attach" => {
            ws.attach_open = true;
            cx.notify();
        }
        "menu" => {
            let id = ws.shown.clone().unwrap_or_default();
            match value {
                "path" => ws.path_menu = true,
                "delete" => {
                    ws.modal = Some(Modal::Delete {
                        thread_id: id,
                        title: "Explain get() Function and Risk".into(),
                    })
                }
                "rename" => {
                    let title = cx.new(|cx| {
                        let mut input = InputState::new(window, cx);
                        input.set_value("Explain get() Function and Risk", window, cx);
                        input
                    });
                    ws.modal = Some(Modal::Rename {
                        thread_id: id,
                        title,
                    })
                }
                "run" => {
                    ws.modal = Some(Modal::RunCode {
                        thread_id: id,
                        command: "cargo test --workspace && cargo clippy".into(),
                    })
                }
                _ => ws.chat_menu = Some((id, point(px(320.), px(300.)))),
            }
            cx.notify();
        }
        "right" => {
            ws.folded.clear();
            ws.right_hidden = false;
            ws.refresh_inspector(cx);
        }
        "fake" if value == "agents" => {
            use dusk_protocol::{
                model::{ModelEvent, ToolCall},
                thread::ThreadEvent,
            };
            let roles = [
                ("backend-engineer", "Add expiry to the cache"),
                ("web-security-specialist", "Check the cache key handling"),
                ("qa-engineer", "Write tests for expiry"),
                ("code-reviewer", "Review the diff of src/lib.rs"),
                ("sre-engineer", "Measure memory growth"),
                ("general", "Summarize the findings"),
            ];
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                for (index, (role, task)) in roles.iter().enumerate() {
                    let thread_id = format!("0000000{index}-fake");
                    chat.apply(ThreadEvent::SubagentStarted {
                        call_id: format!("call-{index}"),
                        thread_id: thread_id.clone(),
                        agent: (*role).into(),
                        task: (*task).into(),
                        effort: None,
                    });
                    let inner = match index {
                        1 => Some(ThreadEvent::ToolStarted(ToolCall {
                            id: "t1".into(),
                            name: "search_files".into(),
                            arguments: serde_json::json!({"pattern": "ttl"}),
                        })),
                        2 => Some(ThreadEvent::Model(ModelEvent::Text("fn expired".into()))),
                        4 => Some(ThreadEvent::Idle),
                        5 => Some(ThreadEvent::Failed("the provider is unavailable".into())),
                        _ => None,
                    };
                    if let Some(event) = inner {
                        chat.apply(ThreadEvent::Subagent {
                            thread_id,
                            event: Box::new(event),
                        });
                    }
                }
            }
            cx.notify();
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
