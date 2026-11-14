//! Development only (`--features screenshot`): with `DUSK_SHOTS` set, the window follows a
//! script and saves pictures of what it draws, rendered in memory, so no permission to
//! record the screen is needed. The script is steps separated by `;`:
//!
//! - `wait=MS`: let the core answer and the window draw;
//! - `project=N`, `thread=N`: choose the Nth project or chat of the lists;
//! - `center=chat|review|settings|connectors|artifacts|schedules`; `review`: open the review page;
//! - `settings=PAGE` (appearance, model, chat, safety, me, notifications, vcs, memory,
//!   advanced, updates, usage), `shelf=NAME` (connectors, skills, plugins, tools, agents),
//!   `artifacts`: open the page as a click would, with its data;
//! - `modal=search|project|schedule|agents|arbiter|whatsnew|none`, `right=all`: open a window; unfold the right column;
//! - `detail=mcp|skill`: open the window of the first of those;
//! - `simulator`: open the iOS Simulator panel, as its button in the header would;
//!   `simulator=devices`: open its menu of devices;
//! - `chat`: show an empty chat that exists only in the window; with `DUSK_METEOR=MS` in
//!   the environment the sky holds a falling star that many milliseconds into its flight;
//! - `fake=agents`: six subagents of different roles at different points of their work;
//!   `fake=agentchat`: open the chat of one, with a run of steps;
//! - `fake=limits`: rate limits in the status, as a provider reports them;
//! - `fake=plugins`: about 200 made-up plugin parts of 60 plugins in the status (see
//!   `dev::plugins`); `plugins=menu`: open the menu of them; `plugins=search:TEXT`: type
//!   in its search; `plugins=open:PLUGIN/PART`: open a part with a made-up view, such as
//!   `ios-simulator/device` (side dock, a phone), `notes/scratch` (bottom dock),
//!   `translator/translate` (window), `telegram/account` (Settings), `plugin-07/part-1`;
//!   `plugins=fold:right|bottom`: fold a dock away or back; `plugins=real:PLUGIN/PART`:
//!   open a part of a real plugin through the core;
//! - `fake=approval|question`: put a request for approval, or a question, in the shown chat;
//! - `picker=model|mode|access|effort|more`, `attach`, `menu=chat|path|delete|rename|run`:
//!   open a menu or window of the chat;
//! - `key=tab tab cmd-k ...`: press keys one after another, as the keyboard would;
//! - `terminal`: show or hide the terminal panel; `run=COMMAND`: type the command in its
//!   shown shell, with Return;
//! - `type=TEXT`: put the text in the message box; `idle=MIN`: as if left alone that long,
//!   for the mist;
//! - `theme=NAME`: Dusk, Light, Dark, "Solar Dusk", Eclipse;
//! - `file=PATH[:LINE]`: open a file in the viewer;
//! - `quiet`: hide the columns as the rail's moon does; `working=MIN`: a turn under way
//!   for that many minutes, for the header's sky;
//! - `shot=NAME`: save `DUSK_SHOT_DIR/NAME.png`; `quit`: end.

use std::{path::PathBuf, time::Duration};

use gpui_kit::{component::input::InputState, *};

use crate::{
    modal::Modal,
    pages::{connectors::Shelf, settings::SettingsPage},
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
                "key" => {
                    for key in value.split_whitespace() {
                        // The parser wants a name that lives forever; a development tool may
                        // leak one. The press runs outside any update of the workspace, as a
                        // real key does.
                        let key: &'static str = Box::leak(key.to_owned().into_boxed_str());
                        match Keystroke::parse(key).ok() {
                            Some(keystroke) => {
                                let _ = cx.update_window(window, |_, window, cx| {
                                    window.dispatch_keystroke(keystroke, cx);
                                });
                            }
                            None => eprintln!("no such key {key}"),
                        }
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
                "schedules" => Center::Schedules,
                _ => Center::Chat,
            };
            if ws.center == Center::Schedules {
                ws.load_schedules(cx);
            }
            cx.notify();
        }
        "review" => ws.open_review(cx),
        "quiet" => ws.quiet = true,
        // A turn of the shown chat that began `value` minutes ago and goes on.
        "working" => {
            let minutes: i64 = value.parse().unwrap_or(7);
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.working = true;
                chat.activity_boundaries.push(chat.items.len());
                chat.boundary_times
                    .push(dusk_protocol::system::now_ms() - minutes * 60_000);
                chat.boundary_spend.push((0, 0));
            }
        }
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
                "keyboard" => SettingsPage::Keyboard,
                "updates" => SettingsPage::Updates,
                "usage" => SettingsPage::Usage,
                "plugins" => SettingsPage::Plugins,
                _ => SettingsPage::Appearance,
            };
            ws.center = Center::Settings;
            ws.load_settings(cx);
            ws.load_page(ws.settings_page, cx);
        }
        "shelf" => {
            ws.shelf = match value {
                "skills" => Shelf::Skills,
                "plugins" => Shelf::Plugins,
                "tools" => Shelf::Tools,
                "voice" => {
                    ws.load_voice_models(cx);
                    Shelf::Voice
                }
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
            "schedule" => ws.open_schedule_editor(None, window, cx),
            // The arbiter's window, with a few things said so its timeline has rows.
            "whatsnew" => {
                ws.modal = Some(crate::modal::Modal::WhatsNew {
                    from: Some("0.73.3".to_owned()),
                });
            }
            "arbiter" => {
                use crate::pages::chat::transcript::{CHAT, Exchange, ExchangeKind};
                let Some(thread_id) = ws.shown.clone() else {
                    return;
                };
                if let Some(chat) = ws.chats.get_mut(&thread_id) {
                    let now = std::time::Instant::now();
                    let said = [
                        (
                            2400,
                            CHAT,
                            "Ember-79",
                            ExchangeKind::Asked,
                            "Second design pass on the dashboard. You must edit src/App.jsx and src/index.css.",
                        ),
                        (
                            1900,
                            "Ember-79",
                            CHAT,
                            ExchangeKind::Reported,
                            "finished:\nStatus: done.\n\nPer file:\n- **tailwind.config.js**: palette replaced with the pass-3 tokens",
                        ),
                        (
                            700,
                            "Ember-79",
                            "arbiter",
                            ExchangeKind::Approval,
                            "run `npm run build` in app/frontend",
                        ),
                        (
                            690,
                            "arbiter",
                            "Ember-79",
                            ExchangeKind::Allowed,
                            "A build inside the project folder changes nothing outside it.",
                        ),
                        (
                            40,
                            "arbiter",
                            "Ember-79",
                            ExchangeKind::Denied,
                            "Deleting the lockfile is outside what was asked.",
                        ),
                    ];
                    for (ago, from, to, kind, text) in said {
                        chat.exchanges.push(Exchange {
                            at: now - Duration::from_secs(ago),
                            from: from.into(),
                            to: to.into(),
                            kind,
                            text: text.into(),
                        });
                    }
                }
                ws.modal = Some(Modal::Arbiter { thread_id });
            }
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
        "idle" => {
            let minutes: u64 = value.parse().unwrap_or(10);
            if let Some(past) =
                std::time::Instant::now().checked_sub(Duration::from_secs(minutes * 60))
            {
                ws.last_input = past;
            }
            cx.notify();
        }
        "dawn" => ws.dawn(cx),
        "terminal" => ws.toggle_terminal(window, cx),
        "run" => ws.type_in_terminal(format!("{value}\r").into_bytes(), cx),
        "type" => {
            ws.fill_composer = Some(value.to_owned());
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
                "voice" => {
                    ws.modal = Some(Modal::VoiceMissing {
                        speech: crate::pages::connectors::voice::Speech::Recognition,
                    })
                }
                "rollback" => {
                    ws.modal = Some(Modal::Rollback {
                        thread_id: id,
                        sequence: 1,
                        at: 0,
                        text: String::new(),
                    })
                }
                _ => ws.chat_menu = Some((id, point(px(320.), px(300.)))),
            }
            cx.notify();
        }
        "simulator" if value == "devices" => ws.toggle_device_menu(cx),
        "simulator" => ws.toggle_simulator(cx),
        "right" => {
            ws.folded.clear();
            ws.right_hidden = false;
            ws.refresh_inspector(cx);
        }
        "fake" if value == "plugins" => {
            ws.status.plugin_parts = std::sync::Arc::new(super::plugins::parts());
            cx.notify();
        }
        "plugins" => {
            let (what, arg) = value.split_once(':').unwrap_or((value, ""));
            match what {
                "menu" => ws.toggle_plugins_menu(cx),
                "open" => super::plugins::open(ws, arg, cx),
                // Through the core, from a real plugin's process.
                "real" => {
                    if let Some((plugin, part)) = arg.split_once('/') {
                        let key = crate::plugins::Key {
                            plugin: plugin.into(),
                            part: part.into(),
                        };
                        ws.open_plugin_part(key, cx);
                    }
                }
                "search" => {
                    let search = ws.plugins.search.clone();
                    search.update(cx, |search, cx| {
                        search.set_value(arg.to_owned(), window, cx)
                    });
                }
                "fold" => {
                    let dock = match arg {
                        "bottom" => &mut ws.plugins.bottom,
                        _ => &mut ws.plugins.right,
                    };
                    dock.collapsed = !dock.collapsed;
                }
                other => eprintln!("no such plugins step: {other}"),
            }
            cx.notify();
        }
        "fake" if value == "limits" => {
            use dusk_protocol::model::{RateLimit, RateLimits};
            ws.status.limits = Some(RateLimits {
                requests: Some(RateLimit {
                    limit: 100,
                    remaining: 35,
                    resets_in: Some(360),
                }),
                tokens: None,
            });
            cx.notify();
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
                ("docs", "Read the old notes"),
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
                        3 => Some(ThreadEvent::ToolStarted(ToolCall {
                            id: "t3".into(),
                            name: "run_command".into(),
                            arguments: serde_json::json!({"command": "cargo test"}),
                        })),
                        4 => Some(ThreadEvent::Idle),
                        5 => Some(ThreadEvent::ToolStarted(ToolCall {
                            id: "t5".into(),
                            name: "read_file".into(),
                            arguments: serde_json::json!({"path": "notes.md"}),
                        })),
                        6 => Some(ThreadEvent::Failed("the provider is unavailable".into())),
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
        "fake" if value == "agentchat" => {
            // A subagent's chat with a run of steps and an answer, in the window.
            use crate::pages::chat::transcript::{Item, Transcript};
            use dusk_protocol::thread::{Tool, ToolState};
            let step = |index: usize, name: &str, detail: &str| {
                Item::Tool(Tool {
                    call_id: format!("c{index}"),
                    name: name.into(),
                    detail: detail.into(),
                    arguments: format!("{{\"path\": \"{detail}\"}}"),
                    output: "ok".into(),
                    state: ToolState::Done,
                })
            };
            let mut inner = Transcript::default();
            inner.items.push(Item::User {
                text: "Add expiry to the cache".into(),
                sequence: Some(1),
            });
            inner.items.push(step(1, "read_file", "src/cache.rs"));
            inner.items.push(step(2, "search_files", "ttl"));
            inner.items.push(step(3, "run_command", "cargo test"));
            inner.items.push(step(4, "run_command", "cargo clippy"));
            inner.items.push(Item::Assistant {
                text: "Entries now expire after the time set in `ttl`; the tests pass.".into(),
                reasoning: String::new(),
            });
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.agent_chats.insert("fake-agent".into(), inner);
            }
            ws.modal = Some(Modal::AgentChat {
                agent: "fake-agent".into(),
                name: "backend-engineer".into(),
            });
            cx.notify();
        }
        "demo" => {
            // A chat that holds a sound card, to look at the player.
            use crate::pages::chat::transcript::{Clip, Item, Transcript};
            let mut chat = Transcript::default();
            chat.items.push(Item::User {
                text: "Say it once more, slower".into(),
                sequence: Some(1),
            });
            chat.items.push(Item::Audio(Clip {
                id: "0192-ab".into(),
                title: Some("scales-voice-b-revised.mp3".into()),
                mime: "audio/mpeg".into(),
                duration: Some(Duration::from_secs(6)),
            }));
            ws.chats.insert("demo".into(), chat);
            ws.shown = Some("demo".into());
            cx.notify();
        }
        // A chat that exists only in the window, so a profile with no chats can show one.
        "chat" => {
            ws.chats.insert("shot".into(), Default::default());
            ws.shown = Some("shot".into());
            ws.center = Center::Chat;
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
        "theme" => crate::ui::theme::change(cx, |prefs| prefs.theme = value.to_owned()),
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
