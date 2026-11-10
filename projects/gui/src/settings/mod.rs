//! The settings page: a menu of categories on the left, the chosen one on the right.

mod appearance;
mod behavior;
mod system;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{widgets, workspace::Workspace};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Model,
    Chat,
    Safety,
    Notifications,
    VersionControl,
    Memory,
    Advanced,
    Updates,
    Usage,
}

const PAGES: &[(SettingsPage, IconName, &str)] = &[
    (SettingsPage::Appearance, IconName::Palette, "Appearance"),
    (SettingsPage::Model, IconName::Cpu, "Model"),
    (SettingsPage::Chat, IconName::Inbox, "Chat"),
    (SettingsPage::Safety, IconName::CircleCheck, "Safety"),
    (SettingsPage::Notifications, IconName::Bell, "Notifications"),
    (
        SettingsPage::VersionControl,
        IconName::Github,
        "Version control",
    ),
    (
        SettingsPage::Memory,
        IconName::MemoryStick,
        "Memory and context",
    ),
    (SettingsPage::Advanced, IconName::Settings2, "Advanced"),
    (SettingsPage::Updates, IconName::RefreshCw, "Updates"),
    (SettingsPage::Usage, IconName::Cpu, "System usage"),
];

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, foreground) = (theme.border, theme.muted_foreground, theme.foreground);
    let mut menu = div()
        .w(px(232.))
        .h_full()
        .flex()
        .flex_col()
        .gap_0p5()
        .p_2()
        .pt_4()
        .border_r_1()
        .border_color(border);
    for (page, icon, label) in PAGES {
        let page = *page;
        menu = menu.child(
            widgets::row(*label, ws.settings_page == page, cx)
                .h(px(36.))
                .child(widgets::icon(icon.clone(), muted))
                .child(div().flex_1().child(*label))
                .when(
                    page == SettingsPage::Updates && ws.status.available_update.is_some(),
                    |row| row.child(widgets::badge("new release!", cx.theme().primary)),
                )
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.settings_page = page;
                    ws.load_page(page, cx);
                    cx.notify();
                })),
        );
    }
    let title = PAGES
        .iter()
        .find(|(page, ..)| *page == ws.settings_page)
        .map_or("", |(.., label)| *label);
    let body = match ws.settings_page {
        SettingsPage::Appearance => appearance::render(ws, window, cx),
        SettingsPage::Updates | SettingsPage::Usage => system::render(ws, ws.settings_page, cx),
        page => behavior::render(ws, page, cx),
    };
    div().size_full().flex().flex_row().child(menu).child(
        div()
            .id("settings-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                div()
                    .max_w(px(760.))
                    .px_8()
                    .py_6()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .text_sm()
                            .text_color(muted)
                            .child("Settings")
                            .child("›")
                            .child(div().text_color(foreground).child(title)),
                    )
                    .child(body),
            ),
    )
}

/// A setting: its name and what it does on the left, its control on the right.
pub fn line(label: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap_0p5()
                .child(label.to_owned())
                .when_hint(hint, theme.muted_foreground),
        )
}

trait Hint {
    fn when_hint(self, hint: &str, color: Hsla) -> Self;
}

impl Hint for Div {
    fn when_hint(self, hint: &str, color: Hsla) -> Self {
        if hint.is_empty() {
            self
        } else {
            self.child(div().text_xs().text_color(color).child(hint.to_owned()))
        }
    }
}
