//! The settings page: a menu of categories on the left, the chosen one on the right.

pub mod about_me;
mod appearance;
mod behavior;
pub mod providers;
mod secrets;
pub use secrets::TokenBox;
mod system;
pub mod uninstall;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{ui::Type, widgets, workspace::Workspace};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Model,
    Chat,
    Safety,
    AboutMe,
    Notifications,
    Keyboard,
    VersionControl,
    Memory,
    Advanced,
    Updates,
    Usage,
    Secrets,
}

const PAGES: &[(SettingsPage, IconName, &str)] = &[
    (SettingsPage::Appearance, IconName::Palette, "Appearance"),
    (SettingsPage::Model, IconName::Cpu, "Model"),
    (SettingsPage::Chat, IconName::Inbox, "Chat"),
    (SettingsPage::Safety, IconName::CircleCheck, "Safety"),
    (SettingsPage::Secrets, IconName::Eye, "Secrets"),
    (SettingsPage::AboutMe, IconName::User, "About me"),
    (SettingsPage::Notifications, IconName::Bell, "Notifications"),
    (SettingsPage::Keyboard, IconName::SquareTerminal, "Keyboard"),
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
    ws.token_box.settle(window, cx);
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
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
        SettingsPage::AboutMe => about_me::render(ws, window, cx),
        SettingsPage::Keyboard => crate::keys::page(cx),
        SettingsPage::Updates | SettingsPage::Usage => system::render(ws, ws.settings_page, cx),
        SettingsPage::Secrets => secrets::render(ws, cx),
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
                    .child(crate::ui::page_title(title, about(ws.settings_page), cx))
                    .child(body),
            ),
    )
}

/// What a page is for, in one line under its title.
fn about(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Appearance => "Theme and fonts, kept on this machine.",
        SettingsPage::Model => "The models the chat can use, and the servers that give them.",
        SettingsPage::Chat => "How a new chat starts: its mode and what it may do.",
        SettingsPage::Safety => "What the agents may do without asking.",
        SettingsPage::AboutMe => "What the agents know about you, and snippets for @names.",
        SettingsPage::Notifications => "How you hear that a chat needs you.",
        SettingsPage::Keyboard => "Shortcuts for the places you go most.",
        SettingsPage::VersionControl => "Branches the agents create, and how they are named.",
        SettingsPage::Memory => "What is remembered between chats, and how much context is kept.",
        SettingsPage::Advanced => "Limits and timings. The defaults suit most work.",
        SettingsPage::Updates => "The version you run and the newest one.",
        SettingsPage::Usage => "What duskcode takes from this machine.",
        SettingsPage::Secrets => "Keys and tokens, kept in the system keychain.",
    }
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
                .child(div().body().child(label.to_owned()))
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
            self.child(div().small().text_color(color).child(hint.to_owned()))
        }
    }
}
