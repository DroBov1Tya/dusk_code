//! Development only (`--features screenshot`): with `DUSK_SHOTS` set, the window follows a
//! script and saves pictures of what it draws, rendered in memory, so no permission to
//! record the screen is needed. The script is steps separated by `;`:
//!
//! - `wait=MS`: let the core answer and the window draw;
//! - `project=N`, `thread=N`: choose the Nth project or chat of the lists;
//! - `center=chat|review|settings|connectors|artifacts|schedules`; `review`: open the review page;
//! - `settings=PAGE` (appearance, llm, voice, image, video, chat, safety, me, notifications, vcs, memory,
//!   advanced, updates, usage), `shelf=NAME` (connectors, skills, plugins, tools, agents),
//!   `artifacts`: open the page as a click would, with its data;
//! - `modal=search|project|schedule|agents|arbiter|whatsnew|none`, `right=all|lower`: open a window; unfold the right column, or only its lower modules;
//! - `detail=mcp|skill`: open the window of the first of those;
//! - with `DUSK_MASCOT=MS` in the environment every subagent's mascot is held that many
//!   milliseconds into its activity, so done and failed can be seen mid-motion;
//! - `chat`: show an empty chat that exists only in the window; with `DUSK_METEOR=MS` in
//!   the environment the sky holds a falling star that many milliseconds into its flight;
//! - `fake=agents`: eight subagents of different roles, one in each state of the mascot;
//!   `fake=agentchat`: open the chat of one, with a run of steps;
//! - `fake=limits`: API rate limits; `fake=plan-limits|plan-unavailable`: ChatGPT plan usage;
//! - `fake=plugins`: about 200 made-up plugin parts of 60 plugins in the status (see
//!   `dev::plugins`); `plugins=menu`: open the menu of them; `plugins=search:TEXT`: type
//!   in its search; `plugins=open:PLUGIN/PART`: open a part with a made-up view, such as
//!   `ios-simulator/phone` (side dock, a phone), `notes/scratch` (bottom dock),
//!   `translator/translate` (window), `telegram/account` (Settings), `plugin-07/part-1`;
//!   `plugins=fold:right|bottom`: fold a dock away or back; `plugins=real:PLUGIN/PART`:
//!   open a part of a real plugin through the core; `plugins=height:PX`: set the bottom
//!   dock's height as a drag would; `plugins=notice:TEXT`: a plugin's note;
//! - `fake=store`: thirty made-up listings in the Workshop (see `dev::store`); `workshop`:
//!   show the Workshop; `workshop=install:NAME|remove:NAME`: its window that asks first;
//!   `workshop=busy:NAME|failed:NAME`: a row at work or after a failure;
//! - `fake=blocks|blocks-working|blocks-wait|blocks-decision`: a chat with a Flow, Checks and
//!   reply buttons, and a turn at work, waiting for the provider, waiting for the user;
//! - `fake=chat|chat-open`: a long chat with folded turns, steps, markdown, a picture, tasks
//!   and pins (see `dev::polish`); `open=pins|plan|panel|done`: the pins drawer, the plan's rows,
//!   the composer's panel, the Todo's finished tasks; `fake=mcp`: five connectors, one of each state; `scroll=N`: the transcript from its Nth row;
//! - `fake=pulls`: four pull requests in Changes, one of each state and checks, a chat of the chosen project in the sidebar with their
//!   icon, and a chat that links two of them, drawn as chips;
//! - `fake=forge-signin|forge-unserved`: the quiet line in Changes when the forge asks to sign in,
//!   or no plugin serves the host;
//! - `fake=approval|approval-long|question`: put a request for approval (a long heredoc
//!   command for `approval-long`), or a question, in the shown chat;
//! - `fake=models`: the chatgpt provider with seven models and one in use, for the model
//!   menu; `fake=sentpicture`: a user's message with a picture thumbnail under it;
//!   `viewer=picture`: the same picture large over the window;
//! - `picker=model|mode|access|effort|more`, `attach`, `menu=chat|path|delete|rename|run`:
//!   open a menu or window of the chat;
//! - `hover=X,Y`: move the pointer to a point of the window; `down=X,Y`, `move=X,Y`,
//!   `up=X,Y`: press the left button there, move with it held, let it go, as a click
//!   (`down` then `up`) or a drag (`down`, a few `move`, `up`) would;
//!   `press=X,Y` is `down` alone, for the pressed look;
//! - `fake=welcome`: no providers, so the chat page shows the welcome; `welcome=form`: its
//!   form, as the button opens it; `welcome=address:URL`: type the address in it;
//! - `key=tab tab cmd-k ...`: press keys one after another, as the keyboard would;
//! - `terminal`: show or hide the terminal panel; `run=COMMAND`: type the command in its
//!   shown shell, with Return; `select=ROW:FROM-TO`: cells of a row selected;
//! - `type=TEXT`: put the text in the message box; `idle=MIN`: as if left alone that long,
//!   for the mist;
//! - `theme=NAME`: Dusk, Light, Dark, "Solar Dusk", Eclipse; `sidebar=project|recent`: how
//!   the side panel lists the chats;
//! - `file=PATH[:LINE]`: open a file in the viewer;
//! - `size=WxH`: resize the window, as a drag of its edge would;
//! - `quiet`: hide the columns as the rail's moon does; `working=MIN`: a turn under way
//!   for that many minutes, for the header's sky;
//! - `wheel=X,Y,DY`: scroll DY pixels at a point (negative goes down);
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
                            // its own: draw the current state, then render that frame. A
                            // draw may ask for the next frame (the Workshop's grid does, once
                            // it has measured its page), so deliver those until none is left,
                            // as the platform's frame loop would.
                            for _ in 0..8 {
                                window.draw(cx).clear(cx);
                                if window.simulate_next_frame(cx) == 0 {
                                    break;
                                }
                            }
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
                // The pointer moved to a point of the window, as the mouse would.
                "hover" => {
                    let at = value
                        .split_once(',')
                        .and_then(|(x, y)| Some(point(px(x.parse().ok()?), px(y.parse().ok()?))));
                    match at {
                        Some(position) => {
                            let _ = cx.update_window(window, |_, window, cx| {
                                window.dispatch_event(
                                    PlatformInput::MouseMove(MouseMoveEvent {
                                        position,
                                        pressed_button: None,
                                        modifiers: Modifiers::default(),
                                    }),
                                    cx,
                                );
                            });
                        }
                        None => eprintln!("hover=X,Y: {value}"),
                    }
                }
                // A turn of the wheel at a point, for what is below a scrolled window's fold.
                "wheel" => {
                    let parts: Vec<f32> = value.split(',').filter_map(|v| v.parse().ok()).collect();
                    let [x, y, dy] = parts[..] else {
                        eprintln!("wheel=X,Y,DY: {value}");
                        continue;
                    };
                    let event = PlatformInput::Scroll(ScrollEvent {
                        position: point(px(x), px(y)),
                        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                        ..Default::default()
                    });
                    let _ = cx.update_window(window, |_, window, cx| {
                        window.draw(cx).clear(cx);
                        window.dispatch_event(event, cx);
                    });
                }
                // The left button pressed, moved with it held, or let go at a point.
                "down" | "press" | "move" | "up" => {
                    let at = value
                        .split_once(',')
                        .and_then(|(x, y)| Some(point(px(x.parse().ok()?), px(y.parse().ok()?))));
                    let Some(position) = at else {
                        eprintln!("{name}=X,Y: {value}");
                        continue;
                    };
                    let event = match name {
                        "down" | "press" => PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        "move" => PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        }),
                        _ => PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        }),
                    };
                    let _ = cx.update_window(window, |_, window, cx| {
                        // The press is hit against the last frame, so draw the state now.
                        window.draw(cx).clear(cx);
                        window.dispatch_event(event, cx);
                    });
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
                "overview" => {
                    ws.overview.back = Some(Center::Chat);
                    Center::Overview
                }
                _ => Center::Chat,
            };
            if ws.center == Center::Schedules {
                ws.load_schedules(cx);
            }
            cx.notify();
        }
        "review" => ws.open_review(cx),
        "quiet" => ws.quiet = true,
        // The window's size, as a drag of its edge would set it.
        "size" => match value.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
            Some((Ok(width), Ok(height))) => window.resize(size(px(width), px(height))),
            _ => eprintln!("size=WIDTHxHEIGHT: {value}"),
        },
        // A turn of the shown chat that began `value` minutes ago and goes on.
        "working" => {
            let minutes: i64 = value.parse().unwrap_or(7);
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.working = true;
                let end = chat.items.len();
                chat.activity_boundaries.push(end);
                chat.boundary_times
                    .push(dusk_protocol::system::now_ms() - minutes * 60_000);
                chat.boundary_spend.push(None);
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
            use crate::pages::settings::ai::Tab;

            let ai_tab = match value {
                "ai" | "llm" | "model" => Some(Tab::Llm),
                "voice" => Some(Tab::Voice),
                "image" => Some(Tab::Image),
                "video" => Some(Tab::Video),
                _ => None,
            };
            ws.settings_page = match value {
                _ if ai_tab.is_some() => SettingsPage::Ai,
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
            if let Some(tab) = ai_tab {
                ws.select_ai_tab(tab, cx);
            } else {
                ws.load_page(ws.settings_page, cx);
            }
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
            // The mode's button reads the modes as it opens its menu; so does this.
            if ws.settings.is_none() {
                ws.load_settings(cx);
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
        // Cells of a row of the shown shell selected, as a drag would: `select=ROW:FROM-TO`.
        "select" => {
            let cells = value.split_once(':').and_then(|(row, cols)| {
                let (from, to) = cols.split_once('-')?;
                Some((row.parse().ok()?, from.parse().ok()?, to.parse().ok()?))
            });
            match cells {
                Some((row, from, to)) => ws.terminal.select_cells(row, from, to),
                None => eprintln!("select=ROW:FROM-TO: {value}"),
            }
            cx.notify();
        }
        "type" => {
            ws.edit_draft(
                ws.shown.clone(),
                crate::pages::chat::composer::Change::Replace(value.to_owned()),
                cx,
            );
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
                        speech: crate::pages::settings::ai::voice::Speech::Recognition,
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
        "right" => {
            use crate::workspace::Right;
            ws.folded.clear();
            // `right=lower`: the tall upper modules folded, so the lower ones are in sight.
            if value == "lower" {
                ws.folded
                    .extend([Right::Overview, Right::Git, Right::Files, Right::Arbiter]);
            }
            ws.right_hidden = false;
            ws.refresh_inspector(cx);
        }
        // A board of 200 tasks in the Overview; the next real answer replaces it.
        "fake" if value == "overview" => {
            ws.overview.chat = ws.shown.clone();
            ws.overview.overview = Some(Default::default());
            ws.overview.plan = Some(dusk_protocol::thread::PlanCounts {
                open: 120,
                claimed: 4,
                running: 6,
                blocked: 10,
                review: 5,
                done: 50,
                failed: 5,
            });
            cx.notify();
        }
        "fake" if value == "pulls" => fake_pulls(ws, cx),
        "fake" if value == "forge-signin" || value == "forge-unserved" => {
            fake_forge(ws, value == "forge-signin", cx)
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
                // As if the bottom dock's edge were dragged to this height.
                "height" => match arg.parse::<f32>() {
                    Ok(height) => ws.plugins.bottom_height = height,
                    Err(_) => eprintln!("plugins=height wants a number: {arg}"),
                },
                "notice" => ws.plugin_notice("notes", arg, cx),
                other => eprintln!("no such plugins step: {other}"),
            }
            cx.notify();
        }
        "fake" if value.starts_with("blocks") => super::blocks::fake(ws, value, cx),
        "fake" if value == "mcp" => super::polish::mcp(ws, cx),
        "fake"
            if matches!(
                value,
                "retry" | "unreachable" | "busy-threads" | "limits-high" | "agent-tree"
            ) =>
        {
            super::polish::state(ws, value, cx)
        }
        "fake" if value.starts_with("chat") => super::polish::fake(ws, value, cx),
        // The transcript scrolled so its Nth row is at the top.
        "scroll" => {
            ws.list.scroll_to(ListOffset {
                item_ix: value.parse().unwrap_or(0),
                offset_in_item: px(0.),
            });
            cx.notify();
        }
        // The chat's folds, as a click on each would open them.
        "open" => {
            match value {
                "pins" => ws.pins.open = true,
                "plan" => ws.dock.plan_open = true,
                "panel" => ws.toggle_panel(cx),
                "done" => ws.todo_done_open = true,
                other => eprintln!("no such fold: {other}"),
            }
            cx.notify();
        }
        "fake" if value == "store" => super::store::fake(ws, cx),
        "workshop" => super::store::step(ws, value, window, cx),
        "fake" if value == "limits" => {
            use dusk_protocol::model::{RateLimit, RateLimits};
            ws.status.limits = Some(RateLimits {
                requests: Some(RateLimit {
                    limit: 100,
                    remaining: 35,
                    resets_in: Some(360),
                }),
                tokens: None,
                plan: Vec::new(),
            });
            cx.notify();
        }
        "fake" if value == "welcome" => {
            ws.providers.clear();
            ws.center = Center::Chat;
            ws.welcome.step = crate::pages::welcome::Step::Intro;
            cx.notify();
        }
        // Two providers, to look at a row's switch and buttons.
        "fake" if value == "providers" => {
            use dusk_protocol::model::ProviderStatus;
            ws.providers = ["chatgpt", "openrouter"]
                .into_iter()
                .map(|name| ProviderStatus {
                    name: name.into(),
                    models: Ok(Vec::new()),
                    balance: None,
                    limits: None,
                    enabled: true,
                    failure: None,
                })
                .collect();
            cx.notify();
        }
        // The chatgpt provider with a list of models and one in use, to look at the model
        // menu: the menu alone shows only the model in use; `picker=more` opens the rest.
        "fake" if value == "models" => {
            use dusk_protocol::model::{Model, ProviderStatus};
            let model = |id: &str| Model {
                id: id.into(),
                context_window: None,
                max_output: None,
            };
            let names = [
                "gpt-5.6-luna",
                "gpt-5.7-nova",
                "gpt-5.8-vega",
                "gpt-6.1-sol",
                "gpt-6.2-rigel",
                "gpt-6-sol",
                "gpt-6.3-atlas",
            ];
            ws.providers = vec![ProviderStatus {
                name: "chatgpt".into(),
                models: Ok(names.into_iter().map(model).collect()),
                balance: None,
                limits: None,
                enabled: true,
                failure: None,
            }];
            ws.status.model = Some("chatgpt/gpt-6.1-sol".into());
            cx.notify();
        }
        // A user's message with a picture thumbnail under it. The bytes are decoded here
        // in place of the core, so a window with no real chat can show the card.
        "fake" if value == "sentpicture" => {
            use crate::pages::chat::messages::SentPicture;
            use crate::pages::chat::transcript::{Item, Transcript};
            let name = "diagram.png";
            let image = std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, demo_png()));
            let thread_id = "shot".to_owned();
            let mut chat = Transcript::default();
            chat.push(
                Item::User {
                    text: format!(
                        "Here is the layout {}",
                        dusk_protocol::thread::source_tag(name)
                    ),
                    sequence: Some(1),
                    answered: Vec::new(),
                },
                0,
            );
            ws.sent_pictures
                .insert((thread_id.clone(), name.into()), SentPicture::Ready(image));
            ws.chats.insert(thread_id.clone(), chat);
            ws.shown = Some(thread_id);
            ws.center = Center::Chat;
            cx.notify();
        }
        // The same picture large over the window, as a click on the thumbnail opens it.
        "viewer" if value == "picture" => {
            let image = std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, demo_png()));
            ws.open_picture(image.into(), window, cx);
        }
        // The ChatGPT plan's windows, as its sign-in reports them.
        "fake" if value == "plan-limits" => {
            use dusk_protocol::model::{PlanWindow, RateLimits};
            ws.status.chatgpt_model = true;
            ws.status.model = Some("chatgpt/gpt-6.1-sol".into());
            ws.status.limits = Some(RateLimits {
                requests: None,
                tokens: None,
                plan: vec![
                    PlanWindow {
                        minutes: Some(300),
                        used_percent: 22,
                        resets_in: Some(7_800),
                    },
                    PlanWindow {
                        minutes: Some(10_080),
                        used_percent: 64,
                        resets_in: Some(259_200),
                    },
                ],
            });
            cx.notify();
        }
        "fake" if value == "plan-unavailable" => {
            ws.status.chatgpt_model = true;
            ws.status.model = Some("chatgpt/gpt-6.1-sol".into());
            ws.status.limits = None;
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
                ("architect", "Plan the cache layout"),
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
                        model: Some("chatgpt/gpt-5.5".into()),
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
                        7 => Some(ThreadEvent::ToolStarted(ToolCall {
                            id: "t7".into(),
                            name: "update_plan".into(),
                            arguments: serde_json::json!({}),
                        })),
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
            use crate::pages::chat::transcript::Item;
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
            // A step with long arguments and a multi-line answer, to show the selectable
            // Arguments and Answer cells; it is left unfolded below.
            let long = Item::Tool(Tool {
                call_id: "c0".into(),
                name: "run_command".into(),
                detail: "cargo test -p cache --all-features".into(),
                arguments:
                    "{\"command\": \"cargo test -p cache --all-features -- --nocapture expiry\", \
                     \"cwd\": \"/Users/me/work/cache\", \"timeout_ms\": 120000}"
                        .into(),
                output: "running 3 tests\ntest ttl::expires_after_deadline ... ok\n\
                         test ttl::keeps_live_entries ... ok\ntest ttl::evicts_on_read ... ok\n\
                         test result: ok. 3 passed; 0 failed"
                    .into(),
                state: ToolState::Done,
            });
            let mut inner = crate::pages::chat::transcript::Log::default();
            inner.push(
                Item::User {
                    text: "Add expiry to the cache".into(),
                    sequence: Some(1),
                    answered: Vec::new(),
                },
                0,
            );
            inner.push(long, 0);
            inner.push(step(2, "read_file", "src/cache.rs"), 0);
            inner.push(step(2, "search_files", "ttl"), 0);
            inner.push(step(3, "run_command", "cargo test"), 0);
            inner.push(step(4, "run_command", "cargo clippy"), 0);
            inner.push(
                Item::Assistant {
                    text: "Entries now expire after the time set in `ttl`; the tests pass.".into(),
                    reasoning: String::new(),
                },
                0,
            );
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.agent_chats.insert("fake-agent".into(), inner);
            }
            // Unfold the run and its first step, so its Arguments and Answer show.
            use crate::pages::chat::messages::{RUN, STEP};
            ws.unfolded.insert(("fake-agent".into(), 1 | RUN));
            ws.unfolded.insert(("fake-agent".into(), 1 | STEP));
            ws.modal = Some(Modal::AgentChat {
                agent: "fake-agent".into(),
                name: "backend-engineer".into(),
            });
            cx.notify();
        }
        "demo" => {
            // A chat that holds a sound card, to look at the player.
            use crate::pages::chat::transcript::{Item, Transcript};
            let mut chat = Transcript::default();
            chat.push(
                Item::User {
                    text: "Say it once more, slower".into(),
                    sequence: Some(1),
                    answered: Vec::new(),
                },
                0,
            );
            chat.push(
                Item::Audio(dusk_protocol::tools::Card {
                    id: "0192-ab".into(),
                    title: Some("scales-voice-b-revised.mp3".into()),
                    mime_type: "audio/mpeg".into(),
                    duration_ms: 6_000,
                    kind: dusk_protocol::tools::CardKind::Audio,
                    ..Default::default()
                }),
                0,
            );
            // A sound the user sent, and work handed to a subagent with its model.
            chat.push(
                Item::User {
                    text: format!(
                        "Compare it with mine {}",
                        dusk_protocol::thread::sound_tag("0192-cd", "/Users/me/voice-take.m4a")
                    ),
                    sequence: Some(2),
                    answered: Vec::new(),
                },
                0,
            );
            chat.push(
                Item::Tool(dusk_protocol::thread::Tool {
                    call_id: "ask-1".into(),
                    name: "ask_agent".into(),
                    detail: "Compare the two takes".into(),
                    arguments: "{}".into(),
                    output: String::new(),
                    state: dusk_protocol::thread::ToolState::Running,
                }),
                0,
            );
            chat.apply(dusk_protocol::thread::ThreadEvent::SubagentStarted {
                call_id: "ask-1".into(),
                thread_id: "0000000a-fake".into(),
                agent: "general".into(),
                task: "Compare the two takes".into(),
                effort: None,
                model: Some("chatgpt/gpt-5.5".into()),
            });
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
                "approval-long" => ThreadEvent::Approval(ApprovalRequest {
                    id: 2,
                    request: Request::Command {
                        command: (0..60).fold(
                            "python3 - <<'EOF'\nimport json, pathlib\n".to_owned(),
                            |text, line| {
                                text + &format!(
                                    "print(json.dumps({{'row': {line}, 'path': str(pathlib.Path.cwd() / 'data' / 'part-{line}.json')}}))\n"
                                )
                            },
                        ) + "EOF",
                    },
                    session: None,
                    steps: Vec::new(),
                }),
                _ => ThreadEvent::Approval(ApprovalRequest {
                    id: 1,
                    request: Request::Command {
                        command: "cargo test --workspace".into(),
                    },
                    session: Some("cargo".into()),
                    steps: Vec::new(),
                }),
            };
            if let Some(chat) = ws.shown.clone().and_then(|id| ws.chats.get_mut(&id)) {
                chat.apply(event);
            }
            cx.notify();
        }
        "theme" => crate::ui::theme::change(cx, |prefs| prefs.theme = value.to_owned()),
        "sidebar" => {
            use crate::columns::sidebar::Order;
            let order = if value == "recent" {
                Order::Recent
            } else {
                Order::Project
            };
            crate::ui::theme::change(cx, |prefs| prefs.chats_by = order);
        }
        "welcome" => match value.split_once(':') {
            Some(("address", address)) => ws.welcome_address(address, window, cx),
            _ => ws.welcome_form(window, cx),
        },
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

/// A small PNG for the picture-thumbnail and viewer shots: a soft gradient with a frame.
pub(super) fn demo_png() -> Vec<u8> {
    use image::{ImageFormat, RgbImage};
    let (w, h) = (640u32, 400u32);
    let mut image = RgbImage::from_fn(w, h, |x, y| {
        let r = 40 + (x * 120 / w) as u8;
        let g = 60 + (y * 120 / h) as u8;
        image::Rgb([r, g, 180])
    });
    for x in 0..w {
        for y in [0, 1, h - 2, h - 1] {
            image.put_pixel(x, y, image::Rgb([230, 230, 240]));
        }
    }
    for y in 0..h {
        for x in [0, 1, w - 2, w - 1] {
            image.put_pixel(x, y, image::Rgb([230, 230, 240]));
        }
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    let _ = image.write_to(&mut bytes, ImageFormat::Png);
    bytes.into_inner()
}

/// A repository whose forge asks to sign in, or has no plugin: the quiet line in Changes.
fn fake_forge(ws: &mut Workspace, sign_in: bool, cx: &mut Context<Workspace>) {
    use dusk_protocol::project::{Forge, ForgeSignIn, RepositoryStatus};
    let host = "github.com".to_owned();
    ws.repository = Some(RepositoryStatus {
        repository: "dusk_code".to_owned(),
        branch: Some("main".to_owned()),
        forge: if sign_in {
            Forge::SignIn(ForgeSignIn {
                host,
                plugin: "github".to_owned(),
                hint: "gh auth login".to_owned(),
            })
        } else {
            Forge::Unserved { host, offer: None }
        },
        ..Default::default()
    });
    cx.notify();
}

/// Pull requests in every state, for the Changes module and a chat row's icon.
fn fake_pulls(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    use dusk_protocol::project::{
        Check, CheckStage, CheckState, Checks, DiffStat, PullState, PullStatus, PullSummary,
        RepositoryStatus,
    };
    use dusk_protocol::project::{Forge, ForgeStatus};
    let Some(project) = ws.selected_project().cloned() else {
        return;
    };
    let now = crate::columns::inspector::checks::now();
    let pull = |number, title: &str, state, checks: Checks| PullStatus {
        number,
        title: title.to_owned(),
        state,
        branch: "pr-status".to_owned(),
        author: "octo-dev".to_owned(),
        created: Some(now - 4 * 3600 - 60 * number as i64),
        updated: Some(now - 600),
        added: 10 * number,
        removed: 7 * number,
        files: Some(number),
        url: format!("https://github.com/example/repo/pull/{number}"),
        checks_url: format!("https://github.com/example/repo/pull/{number}/checks"),
        checks,
    };
    let check = |name: &str, stage, started: i64, finished: Option<i64>| Check {
        name: name.to_owned(),
        workflow: "ci".to_owned(),
        stage,
        url: "https://github.com/example/repo/actions/runs/1".to_owned(),
        started: Some(now - started),
        finished: finished.map(|ago| now - ago),
    };
    // A list as the core gives it: failed, running, queued, passed, skipped.
    let checks = |passed: u32, running: u32, failed: u32, queued: u32, skipped: u32, state| {
        let mut list = Vec::new();
        for n in 0..failed {
            list.push(check(
                &format!("test (shard {n})"),
                CheckStage::Failed,
                400,
                Some(200),
            ));
        }
        for n in 0..running {
            let name = format!("check (target {n})");
            list.push(check(&name, CheckStage::Running, 192 - 60 * n as i64, None));
        }
        for _ in 0..queued {
            list.push(check("web", CheckStage::Queued, 0, None));
        }
        for name in ["fmt", "deny", "clippy"].iter().take(passed as usize) {
            list.push(check(name, CheckStage::Passed, 400, Some(380)));
        }
        for _ in 0..skipped {
            list.push(check("changes", CheckStage::Skipped, 400, Some(398)));
        }
        Checks {
            total: passed + running + failed + queued + skipped,
            passed,
            running,
            failed,
            state: Some(state),
            list,
        }
    };
    let pulls = vec![
        pull(
            12,
            "Show pull requests and CI in Changes",
            PullState::Open,
            // One of each, so the checks window shows every group.
            checks(4, 2, 1, 1, 1, CheckState::Pending),
        ),
        pull(
            11,
            "Draft: a quieter sidebar",
            PullState::Draft,
            Checks::default(),
        ),
        pull(
            9,
            "Split the forge",
            PullState::Merged,
            checks(10, 0, 0, 0, 0, CheckState::Passing),
        ),
        pull(
            7,
            "Bump parking_lot",
            PullState::Closed,
            checks(8, 0, 2, 0, 1, CheckState::Failing),
        ),
    ];
    ws.pulls.insert(
        project.id.clone(),
        PullSummary {
            project_id: project.id.clone(),
            pulls: pulls[..2].to_vec(),
            checks: Some(CheckState::Pending),
        },
    );
    ws.repository = Some(RepositoryStatus {
        repository: "dusk_code".to_owned(),
        branch: Some("pr-status".to_owned()),
        stat: DiffStat {
            files: 23,
            added: 1473,
            removed: 625,
        },
        unavailable: None,
        forge: Forge::Served(Box::new(ForgeStatus {
            plugin: "github".to_owned(),
            name: "GitHub".to_owned(),
            repository: "example/repo".to_owned(),
            pull_prefix: "https://github.com/example/repo/pull/".to_owned(),
            pulls,
            unavailable: None,
        })),
    });
    if let Some(git) = ws.git.as_mut() {
        git.stat = DiffStat {
            files: 23,
            added: 1473,
            removed: 625,
        };
        git.changes = vec![dusk_protocol::project::Change {
            path: "src/lib.rs".to_owned(),
            kind: dusk_protocol::project::ChangeKind::Modified,
            staged: false,
        }];
    }
    ws.threads.insert(
        0,
        dusk_protocol::thread::Thread {
            id: "fake-pulls".to_owned(),
            source: "gui".to_owned(),
            created_at: 0,
            title: Some("Pull request status".to_owned()),
            active_at: 0,
            parent_thread_id: None,
            agent: None,
            name: None,
            project: Some(project.dir.to_string_lossy().into_owned()),
            temporary: false,
            owner: None,
        },
    );
    crate::pages::chat::pull_link::show(ws, cx);
    let mut chat = crate::pages::chat::transcript::Transcript::default();
    chat.extend(
        vec![
            crate::pages::chat::transcript::Item::User {
                text: "Where is https://github.com/example/repo/pull/12 now?".into(),
                sequence: Some(1),
                answered: Vec::new(),
            },
            crate::pages::chat::transcript::Item::Assistant {
                text: "It is open: https://github.com/example/repo/pull/12 builds on \
                   https://github.com/example/repo/pull/9, which is merged. A fork has its own, \
                   https://github.com/someone/fork/pull/3, which stays a plain link."
                    .into(),
                reasoning: String::new(),
            },
        ],
        0,
    );
    let thread_id = ws.shown.clone().unwrap_or_else(|| "shot".to_owned());
    ws.chats.insert(thread_id.clone(), chat);
    ws.shown = Some(thread_id);
    ws.center = crate::workspace::Center::Chat;
    cx.notify();
}
