//! Two pages about the program itself: its updates, and what this window's process uses.

use dusk_protocol::{
    client::{Answer, Command},
    release::InstallStage,
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::{SettingsPage, line, list};
use crate::{
    ui::{Type, controls, widgets},
    workspace::Workspace,
};

const RELEASES: &str = "https://github.com/DroBov1Tya/dusk_code/releases/tag/";

pub fn render(ws: &Workspace, page: SettingsPage, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let body = div().flex().flex_col().gap_6();
    if page == SettingsPage::Usage {
        let Some(report) = &ws.system else {
            return body.child(widgets::muted("Measuring...", cx).px_0());
        };
        let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_048_576.);
        let mut tiles = div().flex().flex_row().flex_wrap().gap_3();
        if let Some(process) = &report.process {
            tiles = tiles
                .child(controls::stat(
                    "Memory",
                    mb(process.resident_bytes),
                    "This window and its core",
                    cx,
                ))
                .child(controls::stat(
                    "CPU",
                    format!("{:.1}%", process.cpu_percent),
                    "Of one core",
                    cx,
                ));
        }
        tiles = tiles.child(controls::stat(
            "Chats",
            report.threads.to_string(),
            &widgets::count(report.messages as usize, "message"),
            cx,
        ));
        let disk = list(
            [
                line("Database", "Chats and their records", cx)
                    .child(figure(mb(report.database_bytes + report.wal_bytes))),
                line("Pictures", "Kept for image cards", cx)
                    .child(figure(mb(report.artifacts_bytes))),
                line("Logs", "Written on this machine only", cx)
                    .child(figure(mb(report.logs_bytes))),
            ],
            cx,
        );
        return body
            .child(tiles)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(widgets::caption("On disk", cx).px_1().pt_0())
                    .child(disk),
            )
            .child(
                div().flex().child(
                    Button::new("usage-refresh")
                        .outline()
                        .icon(IconName::RefreshCw)
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
    let dev = update.channel == dusk_protocol::release::Channel::Dev;
    let tag = update.latest.clone();
    let notes = format!("{RELEASES}{tag}");
    let mut body = body.child(list(
        [
            line(
                "This version",
                &format!("{:?} channel", update.channel).to_lowercase(),
                cx,
            )
            .child(figure(update.current.clone())),
            line("Latest", &format!("{behind} ahead of this one"), cx)
                .child(figure(update.latest.clone())),
            line(
                "Dev builds",
                "Every build of the main branch, before it is released",
                cx,
            )
            .child(
                gpui_kit::component::switch::Switch::new("update-dev")
                    .checked(dev)
                    .on_click(cx.listener(|ws, on: &bool, _, cx| ws.set_channel(*on, cx))),
            ),
            line("Changelog", "What the latest release brings", cx).child(
                Button::new("changelog")
                    .outline()
                    .icon(IconName::ExternalLink)
                    .label("Open")
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&notes))),
            ),
        ],
        cx,
    ));
    if let Some(tag) = ws.update_ready.clone() {
        body = body.child(
            div().flex().items_center().gap_3().child(
                Button::new("update-restart")
                    .primary()
                    .icon(IconName::RefreshCw)
                    .label(format!("Restart into {tag}"))
                    .on_click(cx.listener(|_, _, _, cx| cx.restart())),
            ),
        );
    } else if let Some(progress) = &ws.status.install {
        let stage = match progress.stage {
            InstallStage::Preparing => "Preparing",
            InstallStage::Downloading => "Downloading",
            InstallStage::Verifying => "Checking the download",
            InstallStage::Installing => "Installing",
            InstallStage::Complete => "Installed; restart to use it",
        };
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_1()
                .child(crate::ui::spinner(cx))
                .child(div().small().text_color(muted).child(stage)),
        );
    } else if update.newer && dusk_core::app::running_bundle().is_some() {
        // The app replaces itself: the release's app is checked against its digest and
        // its signature, put in place beside this one, and taken on the next start.
        let install = tag.clone();
        body = body.child(
            div().flex().child(
                Button::new("update-install")
                    .primary()
                    .icon(IconName::ArrowDown)
                    .label(format!("Install {tag}"))
                    .disabled(!update.ready)
                    .tooltip(if update.ready {
                        "Downloads it and puts it in place; restart to use it"
                    } else {
                        "Its build is not ready yet"
                    })
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.install_update_now(install.clone(), cx)),
                    ),
            ),
        );
    } else if update.newer {
        // Not run from the app bundle, as in development: the release page has the app.
        let page = format!("{RELEASES}{tag}");
        body = body.child(
            div().flex().child(
                Button::new("update-download")
                    .primary()
                    .icon(IconName::ExternalLink)
                    .label(format!("Get {tag}"))
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&page))),
            ),
        );
    } else {
        body = body.child(widgets::muted("This is the newest version.", cx).px_1());
    }
    body
}

/// A measured value at a row's right end, in the medium weight so it reads as the answer.
fn figure(text: String) -> Div {
    div().body().font_weight(FontWeight::MEDIUM).child(text)
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
            SettingsPage::AboutMe => self.load_about_me(cx),
            SettingsPage::Model => self.load_models(cx),
            SettingsPage::Secrets => self.load_secrets(cx),
            _ => {}
        }
    }

    /// Switches the update channel and asks again what is newest on it.
    fn set_channel(&mut self, dev: bool, cx: &mut Context<Self>) {
        let channel = if dev { "dev" } else { "stable" }.to_owned();
        self.core.call(
            cx,
            move |client| async move {
                client
                    .command(Command::Update {
                        channel: Some(channel),
                    })
                    .await
            },
            |ws, result, cx| match result {
                Ok(Answer::Update(info)) => {
                    ws.update = Some(info);
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}

impl Workspace {
    /// Installs the release now, on the user's word; the rail and this page then offer
    /// the restart into it.
    fn install_update_now(&mut self, tag: String, cx: &mut Context<Self>) {
        self.update_tried = Some(tag.clone());
        let asked = tag.clone();
        self.core.call(
            cx,
            move |client| async move { client.install_update(&asked).await },
            move |ws, result, cx| {
                match result {
                    Ok(_) => ws.update_ready = Some(tag),
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
    }

    /// Fetches the release the core found, once, when the app can replace itself: the new
    /// copy waits in place and a button in the sidebar restarts into it.
    pub fn fetch_update(&mut self, cx: &mut Context<Self>) {
        let Some(tag) = self.status.available_update.clone() else {
            return;
        };
        if self.status.install.is_some()
            || self.update_tried.as_deref() == Some(tag.as_str())
            || dusk_core::app::running_bundle().is_none()
        {
            return;
        }
        self.update_tried = Some(tag.clone());
        let asked = tag.clone();
        self.core.call(
            cx,
            move |client| async move { client.install_update(&asked).await },
            move |ws, result, cx| {
                match result {
                    Ok(_) => ws.update_ready = Some(tag),
                    Err(error) => tracing::warn!("background update failed: {error:#}"),
                }
                cx.notify();
            },
        );
    }
}
