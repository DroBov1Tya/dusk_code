//! Development only (`--features screenshot`): with `DUSK_SHOTS` set, the window follows a
//! script and saves pictures of what it draws, rendered in memory, so no permission to
//! record the screen is needed. The script is steps separated by `;`:
//!
//! - `wait=MS`: let the core answer and the window draw;
//! - `project=N`, `thread=N`: choose the Nth project or chat of the lists;
//! - `center=chat|review|settings|connectors|artifacts`; `review`: open the review page;
//! - `file=PATH[:LINE]`: open a file in the viewer;
//! - `shot=NAME`: save `DUSK_SHOT_DIR/NAME.png`; `quit`: end.

use std::{path::PathBuf, time::Duration};

use gpui_kit::*;

use crate::workspace::{Center, Workspace};

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
                    workspace.update(cx, |ws, cx| act(ws, &other, &value, cx));
                }
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn act(ws: &mut Workspace, name: &str, value: &str, cx: &mut Context<Workspace>) {
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
