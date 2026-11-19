//! A plugin's Connectors tab: the MCP servers it brings, what each runs or reaches, and
//! whether it is among the connectors here. The words of what one can do are shared with
//! the window that asks before an install.

use dusk_protocol::plugin::{Connector, ConnectorState, Listing, Runs};
use gpui_kit::{component::ActiveTheme, *};

use super::capsule::clip;
use crate::{
    ui::{Type, size},
    workspace::Workspace,
};

const LINE_CHARS: usize = 200;

pub fn render(listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let mut list = div()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border);
    for (index, connector) in listing.connectors.iter().enumerate() {
        let row = row(connector, cx);
        list = list.child(if index == 0 {
            row
        } else {
            row.border_t_1().border_color(cx.theme().border)
        });
    }
    list
}

fn row(connector: &Connector, cx: &App) -> Div {
    let theme = cx.theme();
    let (muted, success, warning) = (theme.muted_foreground, theme.success, theme.warning);
    let (state, color) = match connector.state {
        ConnectorState::NotAdded => ("Not added", muted),
        ConnectorState::Added => ("Added", success),
        ConnectorState::Yours => ("Yours is used", warning),
    };
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(size::GAP_M))
        .p(px(size::GAP_M))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(size::GAP_XS))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .body()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(clip(&connector.name, 64)),
                )
                .child(div().small().text_color(muted).child(can(&connector.runs)))
                .child(
                    div()
                        .small()
                        .font_family(theme.mono_font_family.clone())
                        .child(clip(target(&connector.runs), LINE_CHARS)),
                )
                .children((connector.state == ConnectorState::Yours).then(|| {
                    div().meta().text_color(muted).child(
                        "A connector you set up has this name; yours stays and this one is not used.",
                    )
                })),
        )
        .child(div().flex_none().meta().text_color(color).child(state))
}

/// What a program of a plugin can do.
pub const RUNS_HERE: &str = "Can run a program on this computer";

/// What a server can do, in the words the install window uses.
pub fn can(runs: &Runs) -> &'static str {
    match runs {
        Runs::Command(_) => RUNS_HERE,
        Runs::Url(_) => "Can send data to other services",
    }
}

pub fn target(runs: &Runs) -> &str {
    match runs {
        Runs::Command(line) | Runs::Url(line) => line,
    }
}
