//! A phone drawn around a live screen: a dark body with a metal rim and side buttons, the
//! island over the screen. A click on the screen comes back as a tap, a drag as a swipe,
//! in the screen's own units. The iOS Simulator and plugin screens both draw with it.

use std::{cell::Cell, rc::Rc};

use gpui_kit::*;

/// The bezel at a body 300 px wide; it scales with the body.
const BEZEL: f32 = 11. / 300.;
/// Below this many units a press that moved is still a tap.
const SLOP: f32 = 8.;

/// Where the screen was drawn last, to turn a click into units.
pub type Drawn = Rc<Cell<Option<Bounds<Pixels>>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Tap(u32, u32),
    Swipe((u32, u32), (u32, u32)),
}

/// The bezel of a body this wide.
pub fn bezel(body: f32) -> f32 {
    body * BEZEL
}

/// The width of the screen inside a body this wide.
pub fn inner(body: f32) -> f32 {
    body - 2. * bezel(body)
}

/// The screen of a body `body` wide, `units` wide and tall, showing `content`. The caller
/// adds the click that turns into a gesture.
pub fn screen(
    id: impl Into<ElementId>,
    body: f32,
    units: (f32, f32),
    content: AnyElement,
    drawn: &Drawn,
) -> Stateful<Div> {
    let inner = inner(body);
    let height = inner * units.1 / units.0;
    let corner = px(body * 0.13);
    div()
        .id(id)
        .relative()
        .w(px(inner))
        .h(px(height))
        .rounded(corner)
        .overflow_hidden()
        .bg(rgb(0x000000))
        .child(content)
        .child(measure(drawn))
        .child(island(inner))
}

/// Laid over a screen, notes where it was drawn.
pub fn measure(drawn: &Drawn) -> impl IntoElement {
    let drawn = Rc::clone(drawn);
    canvas(move |bounds, _, _| drawn.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .size_full()
}

/// The widest body, up to `widest`, whose phone fits `width` by `height` for a screen of
/// `units`.
pub fn fit(width: f32, height: f32, units: (f32, f32), widest: f32) -> f32 {
    // A body's height per pixel of its width: the screen's shape plus the bezel.
    let tall = (1. - 2. * BEZEL) * units.1 / units.0 + 2. * BEZEL;
    width.min(widest).min(height / tall).max(1.)
}

/// The body around a screen made by `screen` with the same `body` and `units`.
pub fn body(body: f32, units: (f32, f32), screen: impl IntoElement) -> Div {
    let bezel = bezel(body);
    let height = inner(body) * units.1 / units.0 + 2. * bezel;
    div()
        .relative()
        .flex_none()
        .w(px(body))
        .p(px(bezel))
        .rounded(px(body * 0.17))
        .bg(rgb(0x0b0b0d))
        .border_2()
        .border_color(rgb(0x4a4a50))
        .shadow_lg()
        .child(screen)
        .children(side_buttons(height))
}

/// A picture or a word in the middle of the screen.
pub fn centered(element: impl IntoElement) -> AnyElement {
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

/// What a click on a screen drawn at `drawn` did, in its `units`: the same place is a
/// tap, a move a swipe. None when it was not a mouse's or began or ended off the screen.
pub fn gesture(event: &ClickEvent, drawn: &Drawn, units: (f32, f32)) -> Option<Gesture> {
    let ClickEvent::Mouse(click) = event else {
        return None;
    };
    let bounds = drawn.get()?;
    let from = to_units(bounds, click.down.position, units)?;
    let to = to_units(bounds, click.up.position, units)?;
    let moved = (from.0 as f32 - to.0 as f32).hypot(from.1 as f32 - to.1 as f32);
    Some(if moved < SLOP {
        Gesture::Tap(from.0, from.1)
    } else {
        Gesture::Swipe(from, to)
    })
}

fn to_units(bounds: Bounds<Pixels>, at: Point<Pixels>, units: (f32, f32)) -> Option<(u32, u32)> {
    if !bounds.contains(&at) {
        return None;
    }
    let x = (at.x - bounds.origin.x) / bounds.size.width * units.0;
    let y = (at.y - bounds.origin.y) / bounds.size.height * units.1;
    Some((x.max(0.).round() as u32, y.max(0.).round() as u32))
}
