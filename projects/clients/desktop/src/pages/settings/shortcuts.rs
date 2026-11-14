//! The settings page that lists the keyboard shortcuts, as they read on this platform.

use gpui_kit::*;

use crate::workspace::{Workspace, keys::SHORTCUTS};

/// How a binding reads on this platform: `secondary-alt-b` is ⌘⌥B on a Mac, Ctrl+Alt+B elsewhere.
fn show(binding: &str) -> String {
    let mac = cfg!(target_os = "macos");
    binding
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| match (part, mac) {
            ("secondary", true) => "\u{2318}".to_owned(),
            ("secondary", false) => "Ctrl".to_owned(),
            ("alt", true) => "\u{2325}".to_owned(),
            ("alt", false) => "Alt".to_owned(),
            ("escape", _) => "Esc".to_owned(),
            (other, _) => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join(if mac { "" } else { "+" })
}

/// The settings page that lists the shortcuts.
pub fn page(cx: &mut Context<Workspace>) -> Div {
    use gpui_kit::component::ActiveTheme;
    let (muted, raised) = (cx.theme().muted_foreground, cx.theme().accent);
    let rows = SHORTCUTS.iter().map(|(binding, what)| {
        crate::pages::settings::line(what, "", cx).child(
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(raised)
                .text_color(muted)
                .child(show(binding)),
        )
    });
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(crate::pages::settings::list(rows.collect::<Vec<_>>(), cx));
    body.child(crate::ui::widgets::muted(
        "Escape leaves the message box; then Tab moves through the rows, and Enter or Space presses the one in focus.",
        cx,
    ))
}
