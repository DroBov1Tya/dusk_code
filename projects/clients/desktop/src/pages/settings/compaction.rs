//! Auto-compaction on the Chat page: the switch, and the share of the context window a
//! compaction starts at.

use dusk_protocol::{
    client::Switch as CoreSwitch,
    config::Settings,
    thread::{AUTO_COMPACT_PERCENT, MAX_AUTO_COMPACT_PERCENT, MIN_AUTO_COMPACT_PERCENT},
};
use gpui_kit::{
    component::{
        ActiveTheme,
        slider::{Slider, SliderEvent, SliderState},
        switch::Switch,
    },
    *,
};

use super::line;
use crate::{ui::Type, workspace::Workspace};

/// Shown under the rows while the switch is off.
const OFF_WARNING: &str = "Chats will stop when the context fills up.";

/// The switch row and the slider row, and the warning while it is off.
pub fn render(
    ws: &Workspace,
    settings: &Settings,
    cx: &mut Context<Workspace>,
) -> (Vec<Div>, Option<Div>) {
    let on = settings.auto_compact;
    let switch = line(
        "Auto-compaction",
        "Summarize older messages when the context fills up",
        cx,
    )
    .child(
        Switch::new("auto-compact")
            .checked(on)
            .on_click(cx.listener(|ws, on: &bool, _, cx| {
                ws.switch(CoreSwitch::AutoCompact, *on, cx);
            })),
    );
    let percent = ws.compact_slider.read(cx).value().start().round() as u64;
    let theme = cx.theme();
    let shown = if on {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let slider = line(
        "Compact at",
        "How full the context gets before it is summarized",
        cx,
    )
    .child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w(px(240.))
            .child(
                div().flex_1().child(
                    Slider::new(&ws.compact_slider)
                        .disabled(!on)
                        .accessibility_label("Compact at"),
                ),
            )
            .child(
                div()
                    .w(px(40.))
                    .flex()
                    .justify_end()
                    .body()
                    .text_color(shown)
                    .child(format!("{percent}%")),
            ),
    );
    let warning = (!on).then(|| {
        div()
            .px_1()
            .small()
            .text_color(cx.theme().warning)
            .child(OFF_WARNING)
    });
    (vec![switch, slider], warning)
}

/// The slider at `percent`, saving where it is let go.
pub fn slider(percent: u64, cx: &mut Context<Workspace>) -> Entity<SliderState> {
    let state = cx.new(|_| {
        SliderState::new()
            .min(MIN_AUTO_COMPACT_PERCENT as f32)
            .max(MAX_AUTO_COMPACT_PERCENT as f32)
            .step(1.)
            .default_value(percent as f32)
    });
    cx.subscribe(&state, |ws, _, event: &SliderEvent, cx| match event {
        // The number beside it follows the drag.
        SliderEvent::Change(_) => cx.notify(),
        SliderEvent::Release(value) => {
            ws.set_auto_compact_percent(value.start().round() as u64, cx)
        }
    })
    .detach();
    state
}

/// The default before the core's settings arrive.
pub fn default_slider(cx: &mut Context<Workspace>) -> Entity<SliderState> {
    slider(AUTO_COMPACT_PERCENT, cx)
}

impl Workspace {
    fn set_auto_compact_percent(&mut self, percent: u64, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.settings().auto_compact_percent(percent).await },
            |ws, result, cx| match result {
                Ok(_) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Puts the slider where the core's settings say, when they differ: another client
    /// or the config file may have changed it.
    pub fn follow_compact_percent(&mut self, cx: &mut Context<Self>) {
        let Some(percent) = self.settings.as_ref().map(|s| s.auto_compact_percent) else {
            return;
        };
        let shown = self.compact_slider.read(cx).value().start().round() as u64;
        if shown != percent {
            self.compact_slider = slider(percent, cx);
        }
    }
}
