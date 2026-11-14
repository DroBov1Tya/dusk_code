//! The device drawn as a phone: a dark body with a metal rim and side buttons, the
//! screen inside with the island over it. On the screen a click is a tap and a drag a
//! swipe, given to the device in its points.

use std::rc::Rc;

use dusk_protocol::system::Simulator;
use gpui_kit::{
    component::{
        Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Panel, setup::hint, toolbar};
use crate::workspace::Workspace;

/// The body's width; the screen's height follows the device's own shape.
const BODY: f32 = 300.;
const BEZEL: f32 = 11.;
/// A phone's points before its first picture: an iPhone 17 Pro.
const FIRST: (f32, f32) = (402., 874.);
/// Below this many points a press that moved is still a tap.
const SLOP: f32 = 8.;

pub fn render(panel: &Panel, _found: &Simulator, cx: &mut Context<Workspace>) -> Div {
    let device = panel.chosen_device();
    let (width, height) = panel
        .screen
        .as_ref()
        .map_or(FIRST, |screen| (screen.width, screen.height));
    let inner = BODY - 2. * BEZEL;
    let screen_height = inner * height / width;
    let booted = device.is_some_and(|device| device.booted);
    let corner = px(BODY * 0.13);
    let content = match (&panel.screen, panel.busy, booted) {
        (_, Some(busy), _) => centered(crate::ui::loading(busy, cx)),
        (Some(screen), None, true) => img(ImageSource::Render(screen.picture.clone()))
            .size_full()
            .rounded(corner)
            .object_fit(ObjectFit::Fill)
            .into_any_element(),
        (None, None, true) => centered(hint("Waiting for the screen", cx)),
        (_, None, false) => centered(
            Button::new("simulator-boot")
                .primary()
                .small()
                .label("Boot")
                .on_click(cx.listener(|ws, _, _, cx| ws.power(true, cx))),
        ),
    };
    let drawn = Rc::clone(&panel.drawn);
    let screen = div()
        .id("simulator-screen")
        .relative()
        .w(px(inner))
        .h(px(screen_height))
        .rounded(corner)
        .overflow_hidden()
        .bg(rgb(0x000000))
        .child(content)
        .child(
            canvas(move |bounds, _, _| drawn.set(Some(bounds)), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(island(inner))
        .when(booted && panel.input(), |screen| {
            screen
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, event: &MouseDownEvent, _, cx| {
                        ws.simulator.pressed = points(&ws.simulator, event.position);
                        cx.stop_propagation();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|ws, event: &MouseUpEvent, _, cx| {
                        let from = ws.simulator.pressed.take();
                        let to = points(&ws.simulator, event.position);
                        if let (Some(from), Some(to)) = (from, to) {
                            gesture(ws, from, to, cx);
                        }
                    }),
                )
        });
    let body = div()
        .relative()
        .w(px(BODY))
        .p(px(BEZEL))
        .rounded(px(BODY * 0.17))
        .bg(rgb(0x0b0b0d))
        .border_2()
        .border_color(rgb(0x4a4a50))
        .shadow_lg()
        .child(screen)
        .children(side_buttons(screen_height + 2. * BEZEL));
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_4()
        .child(body)
        .child(toolbar::render(panel, cx))
}

fn centered(element: impl IntoElement) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(element)
        .into_any_element()
}

/// The pill at the top of the screen that the camera sits in.
fn island(inner: f32) -> Div {
    let width = inner * 0.3;
    div()
        .absolute()
        .top(px(inner * 0.03))
        .left(px((inner - width) / 2.))
        .w(px(width))
        .h(px(inner * 0.085))
        .rounded_full()
        .bg(rgb(0x000000))
}

/// The action button and the volume keys on the left, the side button on the right.
fn side_buttons(height: f32) -> Vec<Div> {
    let key = |left: bool, top: f32, length: f32| {
        let key = div()
            .absolute()
            .top(px(height * top))
            .w(px(3.))
            .h(px(height * length))
            .rounded_sm()
            .bg(rgb(0x4a4a50));
        if left {
            key.left(px(-5.))
        } else {
            key.right(px(-5.))
        }
    };
    vec![
        key(true, 0.17, 0.05),
        key(true, 0.26, 0.09),
        key(true, 0.37, 0.09),
        key(false, 0.3, 0.13),
    ]
}

/// A place in the window as the device's points, when it is on the screen.
fn points(panel: &Panel, at: Point<Pixels>) -> Option<(u32, u32)> {
    let bounds = panel.drawn.get()?;
    let screen = panel.screen.as_ref()?;
    if !bounds.contains(&at) {
        return None;
    }
    let x = (at.x - bounds.origin.x) / bounds.size.width * screen.width;
    let y = (at.y - bounds.origin.y) / bounds.size.height * screen.height;
    Some((x.max(0.).round() as u32, y.max(0.).round() as u32))
}

fn gesture(ws: &mut Workspace, from: (u32, u32), to: (u32, u32), cx: &mut Context<Workspace>) {
    let moved = (from.0 as f32 - to.0 as f32).hypot(from.1 as f32 - to.1 as f32);
    if moved < SLOP {
        ws.sim_tap(from.0, from.1, cx);
    } else {
        ws.sim_swipe(from, to, cx);
    }
}
