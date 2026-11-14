//! The files the agents made, in the middle column: by kind (text, pictures, documents
//! and the rest), newest first. Pictures show as thumbnails; any file opens in the viewer.

mod card;
mod menu;

use card::card;
pub use menu::menu;

use dusk_protocol::project::{OutputFile, OutputKind};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const KINDS: &[(Option<OutputKind>, &str)] = &[
    (None, "All"),
    (Some(OutputKind::Text), "Text"),
    (Some(OutputKind::Image), "Images"),
    (Some(OutputKind::Document), "Documents"),
    (Some(OutputKind::Other), "Other"),
];

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (foreground, muted, border) = (theme.foreground, theme.muted_foreground, theme.border);
    let mut tabs = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .px_6()
        .pt_5();
    for (kind, label) in KINDS {
        let kind = *kind;
        let count = ws
            .outputs
            .iter()
            .filter(|file| kind.is_none_or(|kind| file.kind == kind))
            .count();
        let active = ws.artifact_kind == kind;
        tabs = tabs.child(
            widgets::row(*label, active, cx)
                .h(px(crate::ui::size::ROW))
                .gap_2()
                .px_3()
                .body()
                .text_color(if active { foreground } else { muted })
                .child(*label)
                .child(div().meta().text_color(muted).child(count.to_string()))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.artifact_kind = kind;
                    cx.notify();
                })),
        );
    }
    tabs = tabs.child(div().flex_1()).child(
        Button::new("artifacts-refresh")
            .ghost()
            .small()
            .icon(IconName::RefreshCw)
            .tooltip("Read the files again")
            .on_click(cx.listener(|ws, _, _, cx| ws.load_outputs(cx))),
    );
    let files: Vec<OutputFile> = ws
        .outputs
        .iter()
        .filter(|file| ws.artifact_kind.is_none_or(|kind| file.kind == kind))
        .take(300)
        .cloned()
        .collect();
    if !files.is_empty() {
        let paths: Vec<_> = files.iter().map(|file| file.path.clone()).collect();
        tabs = tabs.child(
            Button::new("artifacts-delete-all")
                .ghost()
                .small()
                .icon(IconName::Delete)
                .tooltip("Delete every file shown")
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.modal = Some(crate::modal::Modal::DeleteFiles {
                        paths: paths.clone(),
                    });
                    cx.notify();
                })),
        );
    }
    let mut images = div().flex().flex_wrap().gap_3();
    let mut documents = div().flex().flex_wrap().gap_2();
    let mut has_images = false;
    let mut has_documents = false;
    for (index, file) in files.iter().enumerate() {
        if file.kind == OutputKind::Image {
            has_images = true;
            images = images.child(card(index, file, cx));
        } else {
            has_documents = true;
            documents = documents.child(card(index, file, cx));
        }
    }
    let body = if files.is_empty() {
        crate::ui::empty(
            IconName::LayoutDashboard,
            "No files yet",
            "Files the agents write in the workspace appear here.",
            cx,
        )
        .into_any_element()
    } else {
        let groups = ws.artifact_kind.is_none() && has_images && has_documents;
        let mut items = div().flex().flex_col().gap_3();
        if has_images {
            if groups {
                items = items.child(div().group_name().child("Images"));
            }
            items = items.child(images);
        }
        if has_documents {
            if groups {
                items = items.child(div().group_name().pt_2().child("Files"));
            }
            items = items.child(documents);
        }
        items.into_any_element()
    };
    div()
        .id("artifacts")
        .size_full()
        .overflow_y_scroll()
        .child(tabs)
        .child(div().h(px(1.)).mt_3().bg(border))
        .child(div().p_6().child(body))
}

impl Workspace {
    pub fn load_outputs(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().outputs().await },
            |ws, result, cx| match result {
                Ok(files) => {
                    ws.outputs = files;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }
}
