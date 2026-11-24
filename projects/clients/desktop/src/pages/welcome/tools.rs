//! The Tools page: the tools that stay off until the user switches them on, each with
//! one plain sentence of what it does and what it risks, then how many others are on
//! already, then the way to Voice and to the Workshop. The switches are the core's own,
//! the ones Customize shows; the list is the settings' tool list.

use dusk_protocol::{client::Switch as CoreSwitch, config::ToolInfo};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable, switch::Switch},
    *,
};

use crate::{
    pages::{
        customize::plugins::Scope,
        settings::{line, list},
    },
    ui::{Type, widgets},
    workspace::{Center, Workspace},
};

/// The one tool that moves the user's own pointer; the core keeps it off in every mode
/// until the user switches it on.
const REAL_POINTER: &str = "physical_input";

/// Off until switched on: the screen's tools, and the real pointer whatever its kind.
fn offered(tool: &ToolInfo) -> bool {
    !tool.kind.on_by_default() || tool.name == REAL_POINTER
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let heading = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().title().child("Tools"))
        .child(
            div()
                .body()
                .text_color(muted)
                .child("These stay off until you switch them on."),
        );
    let page = div().flex().flex_col().gap_4().child(heading);
    let Some(settings) = &ws.settings else {
        return page.child(widgets::muted("Loading...", cx));
    };
    let (screen, pointer): (Vec<&ToolInfo>, Vec<&ToolInfo>) = settings
        .tools
        .iter()
        .filter(|tool| offered(tool))
        .partition(|tool| tool.name != REAL_POINTER);
    let mut rows = Vec::new();
    if !screen.is_empty() {
        let names = screen.iter().map(|tool| tool.name.clone()).collect();
        let on = screen.iter().all(|tool| tool.enabled);
        rows.push(
            line(
                "Use the screen",
                "Looks at your screen and clicks, types and scrolls in apps for you. It sees \
                 whatever is shown there, so it asks once each session first.",
                cx,
            )
            .child(toggle("welcome-tool-screen", on, names, cx)),
        );
    }
    if let Some(tool) = pointer.first() {
        rows.push(
            line(
                "Move your real pointer",
                "Clicks with your own mouse pointer when an app ignores the quiet clicks. It \
                 takes your pointer while it acts, so it asks every time.",
                cx,
            )
            .child(toggle(
                "welcome-tool-pointer",
                tool.enabled,
                vec![tool.name.clone()],
                cx,
            )),
        );
    }
    let others = settings
        .tools
        .iter()
        .filter(|tool| !offered(tool) && tool.enabled)
        .count();
    let page = if rows.is_empty() {
        page
    } else {
        page.child(list(rows, cx))
    };
    let verb = if others == 1 { "is" } else { "are" };
    page.child(div().small().text_color(muted).child(format!(
        "{} {verb} already on.",
        widgets::count(others, "other tool")
    )))
    .child(list(
        [
            link(
                "welcome-voice",
                "Voice",
                "Talk to the agent and hear it answer, with speech servers you set up.",
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.open_voice_setup(cx))),
            link(
                "welcome-workshop",
                "Connectors and plugins",
                "More services and features, from the Workshop.",
                cx,
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.center = Center::Customize;
                ws.load_settings(cx);
                ws.discover(Scope::Plugins, cx);
            })),
        ]
        .map(|row| div().child(row)),
        cx,
    ))
}

/// One switch for a group of tools: each is switched in turn, then the settings are
/// read once.
fn toggle(id: &'static str, on: bool, names: Vec<String>, cx: &mut Context<Workspace>) -> Switch {
    Switch::new(id)
        .checked(on)
        .small()
        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
            let (names, on) = (names.clone(), *on);
            ws.core.call(
                cx,
                move |client| async move {
                    let settings = client.settings();
                    for name in names {
                        settings.switch(CoreSwitch::Tool(name), on).await?;
                    }
                    Ok(())
                },
                |ws, result, cx| match result {
                    Ok(()) => ws.load_settings(cx),
                    Err(error) => ws.fail(error, cx),
                },
            );
        }))
}

/// A row that opens another page, with a chevron at its end.
fn link(id: &'static str, label: &str, hint: &str, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    line(label, hint, cx)
        .id(id)
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
        .child(Icon::new(IconName::ChevronRight).small().text_color(muted))
}
