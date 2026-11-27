//! A source of the shown chat opened from the side panel to read: markdown rendered, code
//! in a highlighted block, other text in the mono font, pictures fitted, and anything
//! else by its name and size. The core gives text out in parts by the source's id, never
//! by a path; a long one grows by a part a press, up to `MAX_SHOWN`.

use std::{path::PathBuf, sync::Arc};

use dusk_protocol::thread::{Source, SourceChunk, SourceKind};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Modal;
use crate::{
    pages::chat::links,
    ui::{Type, clipboard, widgets},
    workspace::Workspace,
};

/// The most text the window holds; past it the rest is read in the file's own app.
const MAX_SHOWN: usize = 1024 * 1024;

pub struct SourceView {
    pub thread_id: String,
    pub source: Source,
    pub reading: Reading,
}

pub enum Reading {
    Loading,
    Text {
        text: String,
        next: Option<u64>,
        path: PathBuf,
        /// A further part is on its way.
        more: bool,
    },
    Picture {
        image: Option<Arc<Image>>,
        path: PathBuf,
    },
    Binary {
        path: PathBuf,
    },
    Error(String),
}

pub fn render(view: &SourceView, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let source = &view.source;
    let (kind, path, copied) = match &view.reading {
        Reading::Text { text, path, .. } => ("text", Some(path), Some(text.clone())),
        Reading::Picture { path, .. } => ("picture", Some(path), None),
        Reading::Binary { path } => ("binary file", Some(path), None),
        Reading::Loading | Reading::Error(_) => ("", None, None),
    };
    let about = if kind.is_empty() {
        widgets::short(source.bytes)
    } else {
        format!("{} \u{00B7} {kind}", widgets::short(source.bytes))
    };
    // Only a file on this machine opens here; a core elsewhere keeps its own.
    let local = path.filter(|path| path.is_file()).cloned();
    let copy_id = SharedString::from(format!("source-copy-{}", source.source_id));
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_5()
        .py_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .meta()
                .text_color(muted)
                .child(about),
        )
        .children(copied.map(|text| {
            let more = matches!(view.reading, Reading::Text { next: Some(_), .. });
            Button::new("source-copy")
                .ghost()
                .small()
                .icon(clipboard::icon(&copy_id, cx))
                .label("Copy")
                .tooltip(if more {
                    "Copy the text shown so far"
                } else {
                    "Copy the whole text"
                })
                .on_click(move |_, _, cx| clipboard::copy(&copy_id, text.clone(), cx))
        }))
        .children(local.map(|path| {
            Button::new("source-open")
                .ghost()
                .small()
                .icon(IconName::ExternalLink)
                .label("Open externally")
                .on_click(move |_, _, cx| cx.open_with_system(&path))
        }));
    let body = match &view.reading {
        Reading::Loading => widgets::muted("Loading...", cx).into_any_element(),
        Reading::Error(error) => widgets::muted(error.clone(), cx).into_any_element(),
        Reading::Binary { .. } => widgets::muted(
            "This file is not text, so it is not shown here. Open it in its own app.",
            cx,
        )
        .into_any_element(),
        Reading::Picture { image: None, .. } => widgets::muted("Loading...", cx).into_any_element(),
        Reading::Picture {
            image: Some(image), ..
        } => div()
            .flex()
            .justify_center()
            .child(
                img(image.clone())
                    .max_w_full()
                    .max_h(px(560.))
                    .object_fit(ObjectFit::Contain),
            )
            .into_any_element(),
        Reading::Text {
            text, next, more, ..
        } => {
            let id = SharedString::from(format!("source-text-{}", source.source_id));
            let shown = match look(&source.name) {
                Look::Markdown => {
                    links::markdown(id, text.clone(), links::Links::Web).into_any_element()
                }
                Look::Code(language) => {
                    links::markdown(id, fenced(text, language), links::Links::Web)
                        .into_any_element()
                }
                Look::Plain => div()
                    .font_family(
                        crate::ui::theme::prefs(cx)
                            .mono_font
                            .clone()
                            .unwrap_or_else(|| "Menlo".to_owned()),
                    )
                    .small()
                    .whitespace_normal()
                    .child(text.clone())
                    .into_any_element(),
            };
            let rest = next.map(|next| {
                let left = source.bytes.saturating_sub(next);
                if text.len() >= MAX_SHOWN {
                    widgets::muted(
                        format!(
                            "{} more is not shown here; open the file to read it all.",
                            widgets::short(left)
                        ),
                        cx,
                    )
                    .into_any_element()
                } else {
                    Button::new("source-more")
                        .ghost()
                        .small()
                        .loading(*more)
                        .label(format!("Show more \u{00B7} {} left", widgets::short(left)))
                        .on_click(cx.listener(|ws, _, _, cx| ws.read_more_source(cx)))
                        .into_any_element()
                }
            });
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(shown)
                .children(rest)
                .into_any_element()
        }
    };
    div()
        .flex()
        .flex_col()
        .child(header)
        .child(div().px_5().pb_5().child(body))
}

enum Look {
    Markdown,
    Code(&'static str),
    Plain,
}

/// How a source is drawn, by the end of its name.
fn look(name: &str) -> Look {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let language = match extension.as_str() {
        "md" | "markdown" => return Look::Markdown,
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "jsx" => "javascript",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "hpp" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "sh" | "bash" | "zsh" => "bash",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "html" | "htm" => "html",
        "css" => "css",
        "sql" => "sql",
        "swift" => "swift",
        "kt" => "kotlin",
        _ => return Look::Plain,
    };
    Look::Code(language)
}

/// `text` as one fenced block, its fence longer than any run of backticks inside.
fn fenced(text: &str, language: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{language}\n{text}\n{fence}")
}

impl Workspace {
    /// Opens a source of the shown chat to read; its first part is asked of the core.
    pub fn view_source(&mut self, source: Source, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let source_id = source.source_id.clone();
        self.modal = Some(Modal::Source(SourceView {
            thread_id: thread_id.clone(),
            source,
            reading: Reading::Loading,
        }));
        cx.notify();
        self.read_source(thread_id, source_id, 0, cx);
    }

    fn read_more_source(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Source(view)) = &mut self.modal else {
            return;
        };
        let Reading::Text {
            next: Some(next),
            more,
            ..
        } = &mut view.reading
        else {
            return;
        };
        if *more {
            return;
        }
        *more = true;
        let (thread_id, source_id, next) =
            (view.thread_id.clone(), view.source.source_id.clone(), *next);
        cx.notify();
        self.read_source(thread_id, source_id, next, cx);
    }

    fn read_source(
        &mut self,
        thread_id: String,
        source_id: String,
        offset: u64,
        cx: &mut Context<Self>,
    ) {
        let asked = (thread_id.clone(), source_id.clone());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .work()
                    .source_text(&thread_id, &source_id, offset)
                    .await
            },
            move |ws, result, cx| {
                // An answer for a source closed meanwhile is dropped.
                let Some(Modal::Source(view)) = &mut ws.modal else {
                    return;
                };
                if (view.thread_id.as_str(), view.source.source_id.as_str())
                    != (asked.0.as_str(), asked.1.as_str())
                {
                    return;
                }
                let picture = match result {
                    Ok(chunk) => take(&mut view.reading, chunk),
                    Err(error) => {
                        view.reading = Reading::Error(format!("{error:#}"));
                        false
                    }
                };
                if picture {
                    let (thread_id, name) = (view.thread_id.clone(), view.source.name.clone());
                    ws.read_source_picture(thread_id, name, asked.1, cx);
                }
                cx.notify();
            },
        );
    }

    fn read_source_picture(
        &mut self,
        thread_id: String,
        name: String,
        source_id: String,
        cx: &mut Context<Self>,
    ) {
        self.core.call(
            cx,
            move |client| async move { client.work().source_picture(&thread_id, &name).await },
            move |ws, result, cx| {
                let Some(Modal::Source(view)) = &mut ws.modal else {
                    return;
                };
                if view.source.source_id != source_id {
                    return;
                }
                let Reading::Picture { image, .. } = &mut view.reading else {
                    return;
                };
                match result {
                    Ok(bytes) => {
                        let format = if bytes.starts_with(b"\x89PNG") {
                            ImageFormat::Png
                        } else {
                            ImageFormat::Jpeg
                        };
                        *image = Some(Arc::new(Image::from_bytes(format, bytes)));
                    }
                    Err(error) => view.reading = Reading::Error(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }
}

/// Puts a part the core sent into the reading; true when it is a picture whose bytes are
/// still to be asked for.
fn take(reading: &mut Reading, chunk: SourceChunk) -> bool {
    let path = PathBuf::from(chunk.path);
    match chunk.kind {
        SourceKind::Picture => {
            *reading = Reading::Picture { image: None, path };
            true
        }
        SourceKind::Binary => {
            *reading = Reading::Binary { path };
            false
        }
        SourceKind::Text => {
            match reading {
                Reading::Text {
                    text, next, more, ..
                } => {
                    text.push_str(&chunk.text);
                    *next = chunk.next;
                    *more = false;
                }
                _ => {
                    *reading = Reading::Text {
                        text: chunk.text,
                        next: chunk.next,
                        path,
                        more: false,
                    }
                }
            }
            false
        }
    }
}
