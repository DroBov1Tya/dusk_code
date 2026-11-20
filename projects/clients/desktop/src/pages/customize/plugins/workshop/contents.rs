//! A listing's Contents tab: its files as a tree on the left, the one chosen on the
//! right, Markdown drawn as the chat draws it and other text as it is. The core reads the
//! files, from the copy here or the checked package, and escapes what could hide text; a
//! file that is not text is shown by its size only.

use dusk_protocol::{
    catalog::split_front_matter,
    plugin::{Contents, FilePreview, Listing, PREVIEW_BYTES},
    thread::short_size,
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use super::capsule::clip;
use crate::{
    pages::chat::links,
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The tree's width beside the preview.
const TREE: f32 = 260.;
/// The rows drawn at most; a package may hold thousands of files.
const ROWS: usize = 400;
const NAME_CHARS: usize = 80;

/// Something asked of the core.
#[derive(Debug, Clone, Default)]
pub enum Loaded<T> {
    #[default]
    Loading,
    Ready(T),
    Failed(String),
}

/// The contents of the listing whose window is open.
#[derive(Debug, Clone, Default)]
pub struct Opened {
    pub source: String,
    pub name: String,
    pub files: Loaded<Contents>,
    /// The file in view, by its path.
    pub chosen: Option<String>,
    pub preview: Loaded<FilePreview>,
}

pub fn render(ws: &Workspace, listing: &Listing, width: f32, cx: &mut Context<Workspace>) -> Div {
    let opened = ws
        .workshop
        .contents
        .as_ref()
        .filter(|opened| opened.source == listing.source && opened.name == listing.name);
    let Some(opened) = opened else {
        return widgets::muted("Reading its files...", cx);
    };
    let contents = match &opened.files {
        Loaded::Loading => {
            let what = match listing.installed {
                Some(_) => "Reading its files...",
                None => "Fetching and checking its package...",
            };
            return widgets::muted(what, cx);
        }
        Loaded::Failed(error) => {
            return widgets::muted(format!("Its files could not be read: {error}"), cx);
        }
        Loaded::Ready(contents) => contents,
    };
    let muted = cx.theme().muted_foreground;
    let whence = match contents.installed {
        true => "The copy installed here, with any edits you made.".to_owned(),
        false => format!(
            "The package, checked against its list (SHA-256 {}); nothing of it is installed.",
            short(&contents.package_sha256)
        ),
    };
    let preview_w = (width - TREE - size::GAP_L).max(200.);
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_M))
        .child(div().meta().text_color(muted).child(whence))
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(size::GAP_L))
                .child(tree(contents, opened.chosen.as_deref(), cx))
                .child(preview(opened, preview_w, cx)),
        )
}

/// The files under their folders, each folder once before its files; text files open on
/// a click.
fn tree(contents: &Contents, chosen: Option<&str>, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.accent);
    let mut column = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(TREE))
        .p(px(size::GAP_XS))
        .rounded_lg()
        .border_1()
        .border_color(theme.border);
    let mut folders: Vec<&str> = Vec::new();
    let mut rows = 0;
    for (index, file) in contents.files.iter().enumerate() {
        if rows >= ROWS {
            let left = contents.files.len() - index;
            column = column.child(
                div()
                    .meta()
                    .text_color(muted)
                    .p(px(size::GAP_XS))
                    .child(format!("and {}", widgets::count(left, "file more"))),
            );
            break;
        }
        let parts: Vec<&str> = file.path.split('/').collect();
        let (name, parents) = parts.split_last().unwrap_or((&"", &[]));
        let shared = folders
            .iter()
            .zip(parents.iter())
            .take_while(|(a, b)| a == b)
            .count();
        folders.truncate(shared);
        for (depth, folder) in parents.iter().enumerate().skip(shared) {
            folders.push(folder);
            rows += 1;
            column = column.child(row(depth, format!("{folder}/"), None, cx).text_color(muted));
        }
        rows += 1;
        let size = short_size(file.size);
        let picked = chosen == Some(file.path.as_str());
        let mut item = row(parents.len(), (*name).to_owned(), Some(size), cx)
            .id(SharedString::from(format!("contents-{index}")))
            .when(picked, |item| item.bg(accent));
        if file.text {
            let path = file.path.clone();
            item = item
                .cursor_pointer()
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_contents_file(&path, cx)));
        } else {
            item = item.text_color(muted);
        }
        column = column.child(item);
    }
    if contents.files.is_empty() {
        column = column.child(widgets::muted("It holds no files.", cx));
    }
    column
}

fn row(depth: usize, name: String, size: Option<String>, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .pl(px(size::GAP_S + 12. * depth.min(8) as f32))
        .pr(px(size::GAP_S))
        .py(px(2.))
        .rounded_md()
        .small()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(clip(&name, NAME_CHARS)),
        )
        .when_some(size, |row, size| {
            row.child(
                div()
                    .flex_none()
                    .meta()
                    .text_color(cx.theme().muted_foreground)
                    .child(size),
            )
        })
}

/// The chosen file, or what to do first.
fn preview(opened: &Opened, width: f32, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let frame = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .w(px(width))
        .min_w_0();
    let Some(path) = &opened.chosen else {
        return frame.child(widgets::muted(
            "Choose a file to read it. Programs and other files that are not text show their size only.",
            cx,
        ));
    };
    let frame = frame.child(
        div()
            .small()
            .font_family(theme.mono_font_family.clone())
            .text_color(muted)
            .child(clip(path, 200)),
    );
    match &opened.preview {
        Loaded::Loading => frame.child(widgets::muted("Reading...", cx)),
        Loaded::Failed(error) => frame.child(widgets::muted(error.clone(), cx)),
        Loaded::Ready(file) => {
            let plain = |text: String| {
                div()
                    .font_family(theme.mono_font_family.clone())
                    .small()
                    .whitespace_normal()
                    .child(text)
            };
            let body = match path.ends_with(".md") {
                // Front matter is data, not Markdown: as Markdown its fences read as a
                // heading.
                true => {
                    let (front, rest) = front_matter(&file.text);
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(size::GAP_S))
                        .when_some(front, |body, front| {
                            body.child(plain(front.to_owned()).text_color(muted))
                        })
                        .child(links::markdown(
                            "contents-markdown",
                            rest.to_owned(),
                            links::Links::Web,
                        ))
                }
                false => plain(file.text.clone()),
            };
            frame.child(body).when(file.truncated, |frame| {
                frame.child(div().meta().text_color(muted).child(format!(
                    "Only the first {} are shown.",
                    short_size(PREVIEW_BYTES as u64)
                )))
            })
        }
    }
}

/// The `---` block a skill's file opens with, fences included, and the rest.
fn front_matter(text: &str) -> (Option<&str>, &str) {
    match split_front_matter(text) {
        Ok((_, body)) => {
            let block = &text[..text.len() - body.len()];
            (Some(block.trim_end_matches(['\r', '\n'])), body)
        }
        Err(_) => (None, text),
    }
}

/// The first twelve hex digits of a hash, enough to compare by eye.
fn short(sha256: &str) -> &str {
    sha256.get(..12).unwrap_or(sha256)
}

impl Workspace {
    /// Asks the core for the files of `listing`, once per window opened.
    pub fn open_contents(&mut self, listing: &Listing, cx: &mut Context<Self>) {
        let already = self.workshop.contents.as_ref().is_some_and(|opened| {
            opened.source == listing.source
                && opened.name == listing.name
                && !matches!(opened.files, Loaded::Failed(_))
        });
        if already {
            return;
        }
        let (source, name) = (listing.source.clone(), listing.name.clone());
        self.workshop.contents = Some(Opened {
            source: source.clone(),
            name: name.clone(),
            ..Opened::default()
        });
        self.core.call(
            cx,
            move |client| async move { client.plugins().contents(&source, &name).await },
            |ws, result, cx| {
                if let Some(opened) = &mut ws.workshop.contents {
                    opened.files = match result {
                        Ok(contents) => Loaded::Ready(contents),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
                    };
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Asks the core for one text file of the open contents.
    pub fn open_contents_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(opened) = &mut self.workshop.contents else {
            return;
        };
        opened.chosen = Some(path.to_owned());
        opened.preview = Loaded::Loading;
        let (source, name, path) = (opened.source.clone(), opened.name.clone(), path.to_owned());
        let asked = path.clone();
        self.core.call(
            cx,
            move |client| async move { client.plugins().contents_file(&source, &name, &path).await },
            move |ws, result, cx| {
                if let Some(opened) = &mut ws.workshop.contents
                    && opened.chosen.as_deref() == Some(asked.as_str())
                {
                    opened.preview = match result {
                        Ok(file) => Loaded::Ready(file),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
                    };
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

/// Whether a listing has files to show: it is here, or a package is offered for this
/// machine.
pub fn has_files(listing: &Listing) -> bool {
    listing.installed.is_some() || listing.unfit.is_none()
}
