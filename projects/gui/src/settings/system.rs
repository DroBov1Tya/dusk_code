//! Two pages about the program itself: its updates, and what this window's process uses.

use dusk_protocol::{
    command::{Answer, Command},
    update::InstallStage,
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{SettingsPage, line};
use crate::{widgets, workspace::Workspace};

const RELEASES: &str = "https://github.com/DroBov1Tya/dusk_code/releases/tag/";

pub fn render(ws: &Workspace, page: SettingsPage, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut body = div().flex().flex_col();
    if page == SettingsPage::Usage {
        let Some(report) = &ws.system else {
            return body.child(widgets::muted("Measuring...", cx).px_0());
        };
        let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_048_576.);
        if let Some(process) = &report.process {
            body = body
                .child(
                    line(
                        "Memory",
                        "Resident memory of this window and the core inside it",
                        cx,
                    )
                    .child(mb(process.resident_bytes)),
                )
                .child(
                    line("CPU", "Of one core", cx).child(format!("{:.1}%", process.cpu_percent)),
                );
        }
        return body
            .child(
                line("Database", "Chats and their records", cx)
                    .child(mb(report.database_bytes + report.wal_bytes)),
            )
            .child(line("Pictures", "Kept for image cards", cx).child(mb(report.artifacts_bytes)))
            .child(line("Logs", "", cx).child(mb(report.logs_bytes)))
            .child(
                line("Chats", &format!("{} messages", report.messages), cx)
                    .child(report.threads.to_string()),
            )
            .child(
                div().pt_3().child(
                    Button::new("usage-refresh")
                        .small()
                        .label("Measure again")
                        .on_click(
                            cx.listener(|ws, _, _, cx| ws.load_page(SettingsPage::Usage, cx)),
                        ),
                ),
            );
    }
    let Some(update) = &ws.update else {
        return body.child(widgets::muted("Asking GitHub...", cx).px_0());
    };
    let behind = update
        .commits_behind
        .map_or("unknown".to_owned(), |count| format!("{count} commits"));
    body = body
        .child(
            line(
                "This version",
                &format!("{:?} channel", update.channel).to_lowercase(),
                cx,
            )
            .child(update.current.clone()),
        )
        .child(
            line("Latest", &format!("{behind} ahead of this one"), cx).child(update.latest.clone()),
        );
    let tag = update.latest.clone();
    let notes = format!("{RELEASES}{tag}");
    body = body.child(
        line("Changelog", "What the latest release brings", cx).child(
            Button::new("changelog")
                .ghost()
                .small()
                .label("Open")
                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&notes))),
        ),
    );
    if let Some(progress) = &ws.status.install {
        let stage = match progress.stage {
            InstallStage::Preparing => "Preparing",
            InstallStage::Downloading => "Downloading",
            InstallStage::Verifying => "Verifying",
            InstallStage::Installing => "Installing",
            InstallStage::Complete => "Installed; restart to use it",
        };
        body = body.child(div().pt_3().text_sm().text_color(muted).child(stage));
    } else if update.newer {
        body = body.child(
            div().pt_3().child(
                Button::new("update-install")
                    .primary()
                    .label(format!("Update to {tag}"))
                    .disabled(!update.ready)
                    .tooltip(if update.ready {
                        "Download, verify and install it"
                    } else {
                        "Its build is not ready yet"
                    })
                    .on_click(cx.listener(move |ws, _, _, cx| ws.install(tag.clone(), cx))),
            ),
        );
    } else {
        body = body.child(widgets::muted("This is the newest version.", cx).px_0());
    }
    body
}

impl Workspace {
    /// Asks the core for what a settings page shows.
    pub fn load_page(&mut self, page: SettingsPage, cx: &mut Context<Self>) {
        match page {
            SettingsPage::Usage => self.core.call(
                cx,
                |client| async move { client.system_report().await },
                |ws, result, cx| match result {
                    Ok(report) => {
                        ws.system = Some(report);
                        cx.notify();
                    }
                    Err(error) => ws.fail(error, cx),
                },
            ),
            SettingsPage::Updates => self.core.call(
                cx,
                |client| async move { client.command(Command::Update { channel: None }).await },
                |ws, result, cx| match result {
                    Ok(Answer::Update(info)) => {
                        ws.update = Some(info);
                        cx.notify();
                    }
                    Ok(_) => {}
                    Err(error) => ws.fail(error, cx),
                },
            ),
            _ => {}
        }
    }

    fn install(&mut self, tag: String, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.install_update(&tag).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
