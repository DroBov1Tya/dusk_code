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
    component::{ActiveTheme, Colorize, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

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

type Page = (SettingsPage, IconName, &'static str);

/// The pages in the order they are reached for: what the agent is and may do first, then
/// how the window looks and speaks, then what it keeps, then the machine. Each group has
/// a quiet name; removal sits at the very end, under Advanced.
const GROUPS: &[(&str, &[Page])] = &[
    (
        "Agent",
        &[
            (SettingsPage::Model, IconName::Cpu, "Model"),
            (SettingsPage::Chat, IconName::Inbox, "Chat"),
            (SettingsPage::Safety, IconName::CircleCheck, "Safety"),
            (SettingsPage::Memory, IconName::MemoryStick, "Memory"),
            (SettingsPage::AboutMe, IconName::User, "About me"),
        ],
    ),
    (
        "Window",
        &[
            (SettingsPage::Appearance, IconName::Palette, "Appearance"),
            (SettingsPage::Notifications, IconName::Bell, "Notifications"),
            (SettingsPage::Keyboard, IconName::SquareTerminal, "Keyboard"),
        ],
    ),
    (
        "Data",
        &[
            (SettingsPage::Secrets, IconName::Eye, "Secrets"),
            (
                SettingsPage::VersionControl,
                IconName::Github,
                "Version control",
            ),
        ],
    ),
    (
        "System",
        &[
            (SettingsPage::Updates, IconName::RefreshCw, "Updates"),
            (SettingsPage::Usage, IconName::ChartPie, "System usage"),
            (SettingsPage::Advanced, IconName::Settings2, "Advanced"),
        ],
    ),
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
    let pages = || GROUPS.iter().flat_map(|(_, pages)| pages.iter());
    for (index, (group, _)) in GROUPS.iter().enumerate() {
        menu = menu.child(
            div()
                .px_2()
                .pb_1()
                .when(index > 0, |name| name.pt_3())
                .meta()
                .text_color(muted.opacity(0.8))
                .child(*group),
        );
        menu = menu.children(GROUPS[index].1.iter().map(|(page, icon, label)| {
            let page = *page;

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
                }))
        }));
    }
    let title = pages()
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
                // A reading width in the middle of the page, not a strip along its left.
                div()
                    .max_w(px(880.))
                    .mx_auto()
                    .px_8()
                    .py_8()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(crate::ui::page_title(title, about(ws.settings_page), cx))
                    .child(body),
            ),
    )
}

/// What a page is for, in one line under its title.
fn about(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Appearance => "Theme and fonts, kept on this machine.",
        SettingsPage::Model => "The model in use, how much context it gets, and its providers.",
        SettingsPage::Chat => "How a new chat starts: its mode and what it may do.",
        SettingsPage::Safety => "What the agents may do without asking.",
        SettingsPage::AboutMe => "What the agents know about you, and snippets for @names.",
        SettingsPage::Notifications => "How you hear that a chat needs you.",
        SettingsPage::Keyboard => "Shortcuts for the places you go most.",
        SettingsPage::VersionControl => "Branches the agents create, and how they are named.",
        SettingsPage::Memory => "What is remembered between chats.",
        SettingsPage::Advanced => "Limits and timings. The defaults suit most work.",
        SettingsPage::Updates => "The version you run and the newest one.",
        SettingsPage::Usage => "What duskcode takes from this machine.",
        SettingsPage::Secrets => "Keys and tokens, kept in the system keychain.",
    }
}

/// A setting: its name and what it does on the left, its control on the right. Rows
/// stand in a `list`, which draws the card and the lines between them.
pub fn line(label: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .min_h(px(56.))
        .px_4()
        .py_2p5()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().body().child(label.to_owned()))
                .when_hint(hint, theme.muted_foreground),
        )
}

/// Rows of settings grouped on one card, as macOS lists them: a hairline between two
/// rows, none above the first or under the last.
pub fn list(rows: impl IntoIterator<Item = Div>, cx: &App) -> Div {
    let theme = cx.theme();
    let border = theme.border;
    div()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(theme.muted)
        .overflow_hidden()
        .children(rows.into_iter().enumerate().map(move |(index, row)| {
            row.when(index > 0, |row| row.border_t_1().border_color(border))
        }))
}

fn section(title: &str, hint: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_xl()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .body()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_owned()),
                )
                .when_hint(hint, section_hint_color(cx)),
        )
}

fn section_hint_color(cx: &App) -> Hsla {
    let theme = cx.theme();
    theme.foreground.mix_oklab(theme.muted_foreground, 0.16)
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
