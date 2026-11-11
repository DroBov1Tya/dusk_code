//! A file opened from the outputs or the artifacts: markdown rendered, other text in
//! the mono font, pictures drawn. The core reads text, only inside its own folders and
//! the projects; pictures are drawn from the path the core listed.

use std::path::{Path, PathBuf};

use dusk_protocol::project::OutputKind;
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Modal;
use crate::{pages::chat::links, ui::widgets, workspace::Workspace};

pub enum ViewerContent {
    Loading,
    Markdown(String),
    Text(String),
    Image,
    /// A kind the window does not draw, such as a PDF; the system opens it.
    External,
    Error(String),
}

pub fn render(
    path: &Path,
    content: &ViewerContent,
    _: &mut Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let shown = path.to_path_buf();
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
                .text_xs()
                .text_color(muted)
                .child(path.display().to_string()),
        )
        .child(
            Button::new("viewer-open")
                .ghost()
                .xsmall()
                .icon(IconName::ExternalLink)
                .label("Open in the system")
                .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&shown))),
        );
    let body = match content {
        ViewerContent::Loading => widgets::muted("Loading...", cx).into_any_element(),
        ViewerContent::Markdown(text) => {
            links::markdown("viewer-markdown", text.clone()).into_any_element()
        }
        ViewerContent::Text(text) => div()
            .font_family(
                crate::ui::theme::prefs(cx)
                    .mono_font
                    .clone()
                    .unwrap_or_else(|| "Menlo".to_owned()),
            )
            .text_sm()
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        ViewerContent::Image => img(path.to_path_buf())
            .max_w_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        ViewerContent::External => {
            widgets::muted("This kind of file opens in its own app.", cx).into_any_element()
        }
        ViewerContent::Error(error) => widgets::muted(error.clone(), cx).into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .child(header)
        .child(div().px_5().pb_5().child(body))
}

impl Workspace {
    /// Opens a file in the viewer; text is asked of the core.
    pub fn view_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let kind = kind(&path);
        let content = match kind {
            OutputKind::Image => ViewerContent::Image,
            OutputKind::Document | OutputKind::Other if !is_text_like(&path) => {
                ViewerContent::External
            }
            _ => ViewerContent::Loading,
        };
        let loading = matches!(content, ViewerContent::Loading);
        self.modal = Some(Modal::Viewer {
            path: path.clone(),
            content,
        });
        cx.notify();
        if !loading {
            return;
        }
        let markdown = path.extension().is_some_and(|ext| ext == "md");
        let asked = path.clone();
        self.core.call(
            cx,
            move |client| async move { client.projects().read_output(asked).await },
            move |ws, result, cx| {
                if let Some(Modal::Viewer {
                    path: open,
                    content,
                }) = &mut ws.modal
                    && *open == path
                {
                    *content = match result {
                        Ok(text) if markdown => ViewerContent::Markdown(text),
                        Ok(text) => ViewerContent::Text(text),
                        Err(error) => ViewerContent::Error(format!("{error:#}")),
                    };
                    cx.notify();
                }
            },
        );
    }
}

fn is_text_like(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "csv" || ext == "tsv")
}

pub fn kind(path: &Path) -> OutputKind {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => OutputKind::Image,
        "pdf" | "docx" | "doc" | "odt" | "rtf" | "csv" | "tsv" | "xlsx" | "xls" | "pptx"
        | "epub" => OutputKind::Document,
        "" => OutputKind::Other,
        _ => OutputKind::Text,
    }
}
