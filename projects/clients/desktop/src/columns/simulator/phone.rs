//! The device drawn as a phone (`ui::phone`), its screen inside. On the screen a click is
//! a tap and a drag a swipe, given to the device in its points.

use dusk_protocol::system::Simulator;
use gpui_kit::{
    component::button::{Button, ButtonVariants},
    prelude::FluentBuilder,
    *,
};

use super::{Panel, setup::hint, toolbar};
use crate::{
    ui::phone::{self, Gesture},
    workspace::Workspace,
};

/// The body's width; the screen's height follows the device's own shape.
const BODY: f32 = 300.;
/// A phone's points before its first picture: an iPhone 17 Pro.
const FIRST: (f32, f32) = (402., 874.);

pub fn render(panel: &Panel, _found: &Simulator, cx: &mut Context<Workspace>) -> Div {
    let device = panel.chosen_device();
    let units = panel
        .screen
        .as_ref()
        .map_or(FIRST, |screen| (screen.width, screen.height));
    let booted = device.is_some_and(|device| device.booted);
    let content = match (&panel.screen, panel.busy, booted) {
        (_, Some(busy), _) => phone::centered(crate::ui::loading(busy, cx)),
        (Some(screen), None, true) => img(ImageSource::Render(screen.picture.clone()))
            .size_full()
            .rounded(px(BODY * 0.13))
            .object_fit(ObjectFit::Fill)
            .into_any_element(),
        (None, None, true) => phone::centered(hint("Waiting for the screen", cx)),
        (_, None, false) => phone::centered(
            Button::new("simulator-boot")
                .primary()
                .label("Boot")
                .on_click(cx.listener(|ws, _, _, cx| ws.power(true, cx))),
        ),
    };
    let screen = phone::screen("simulator-screen", BODY, units, content, &panel.drawn).when(
        booted && panel.input(),
        |screen| {
            screen
                .cursor_pointer()
                .on_click(cx.listener(|ws, event: &ClickEvent, _, cx| {
                    let Some(screen) = ws.simulator.screen.as_ref() else {
                        return;
                    };
                    let units = (screen.width, screen.height);
                    match phone::gesture(event, &ws.simulator.drawn, units) {
                        Some(Gesture::Tap(x, y)) => ws.sim_tap(x, y, cx),
                        Some(Gesture::Swipe(from, to)) => ws.sim_swipe(from, to, cx),
                        None => {}
                    }
                }))
        },
    );
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_4()
        .child(phone::body(BODY, units, screen))
        .child(toolbar::render(panel, cx))
}
