//! A failure said quietly: the words in the text color on a raised row, a thin edge and
//! an icon in the danger color to say what kind of row it is. The page stays calm; a red
//! slab across the column was louder than a call that can simply be sent again.

use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    ui::{
        Type,
        elevation::{Elevation, Level},
        widgets,
    },
    workspace::Workspace,
};

/// `again`: the chat to send the last message of once more, for the error the chat ends
/// with while no turn runs.
pub fn error_row(
    key: &str,
    text: &str,
    again: Option<String>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let danger = theme.danger;
    let group = SharedString::from(format!("error-row-{key}"));
    div()
        .group(group.clone())
        .relative()
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(crate::ui::size::ROW))
        .pl_4()
        .pr_2()
        .py_1p5()
        .rounded_lg()
        .overflow_hidden()
        .elevation(Level::Raised, cx)
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(2.))
                .bg(danger),
        )
        .child(
            Icon::new(IconName::CircleAlert)
                .size(px(crate::ui::size::ICON))
                .flex_none()
                .text_color(danger),
        )
        // Selectable: an error's code or path is often wanted without the rest.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .small()
                .child(gpui_kit::base::SelectableText::new(
                    SharedString::from(format!("error-text-{key}")),
                    text.to_owned(),
                )),
        )
        .child(widgets::copy(
            SharedString::from(format!("error-copy-{key}")),
            &group,
            text,
            cx,
        ))
        .children(again.map(|thread_id| {
            Button::new(SharedString::from(format!("error-again-{key}")))
                .ghost()
                .small()
                .icon(IconName::Redo2)
                .label("Send again")
                .tooltip("Send your last message once more")
                .on_click(cx.listener(move |ws, _, _, cx| ws.send_again(&thread_id, cx)))
        }))
        .into_any_element()
}
