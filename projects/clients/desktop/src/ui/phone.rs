//! A phone drawn around a live screen: a thin black bezel in a light metal rim, side
//! buttons of the same metal, the island over the screen. Presses, drags and the wheel on the screen
//! come back in the screen's own units. Plugin screens with `phone` draw with it.

use std::{cell::Cell, rc::Rc};

use gpui_kit::*;

/// The bezel at a body 300 px wide; it scales with the body.
const BEZEL: f32 = 10. / 300.;
/// The screen's corner per pixel of the body's width; the body's adds the bezel, so the
/// two curves run side by side.
pub const CORNER: f32 = 0.13;
/// The rim's width in pixels (`border_2`), inside the body's width.
const RIM: f32 = 2.;
/// The rim and the side buttons: a light metal that reads on dark and light themes.
const METAL: u32 = 0x8e8e93;

/// Where the screen was drawn last, to turn a click into units.
pub type Drawn = Rc<Cell<Option<Bounds<Pixels>>>>;

/// The bezel of a body this wide.
pub fn bezel(body: f32) -> f32 {
    body * BEZEL
}

/// The width of the screen inside a body this wide.
pub fn inner(body: f32) -> f32 {
    body - 2. * (bezel(body) + RIM)
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
    let corner = px(body * CORNER);
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
    // A body's height is `tall` per pixel of its width plus the rims, which do not
    // scale: the screen's shape between them, the bezel and the rim around it.
    let shape = units.1 / units.0;
    let tall = (1. - 2. * BEZEL) * shape + 2. * BEZEL;
    let rims = 2. * RIM * (1. - shape);
    width.min(widest).min((height - rims) / tall).max(1.)
}

/// The body around a screen made by `screen` with the same `body` and `units`.
pub fn body(body: f32, units: (f32, f32), screen: impl IntoElement) -> Div {
    let bezel = bezel(body);
    let height = inner(body) * units.1 / units.0 + 2. * (bezel + RIM);
    div()
        .relative()
        .flex_none()
        .w(px(body))
        .p(px(bezel))
        .rounded(px(body * (CORNER + BEZEL)))
        .bg(rgb(0x050506))
        .border_2()
        .border_color(rgb(METAL))
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
            .w(px(2.))
            .h(px(height * length))
            .rounded_sm()
            .bg(rgb(METAL));
        if left {
            key.left(px(-4.))
        } else {
            key.right(px(-4.))
        }
    };
    vec![
        key(true, 0.17, 0.05),
        key(true, 0.26, 0.09),
        key(true, 0.37, 0.09),
        key(false, 0.3, 0.13),
    ]
}

/// The place of `at` on a screen drawn at `drawn`, in its `units`; `None` off the screen.
pub fn units_at(drawn: &Drawn, at: Point<Pixels>, units: (f32, f32)) -> Option<(u32, u32)> {
    let bounds = drawn.get()?;
    bounds
        .contains(&at)
        .then(|| clamped(bounds, at, units))
        .flatten()
}

/// The place of `at` held to the screen drawn at `drawn`, for a drag that left it.
pub fn units_held(drawn: &Drawn, at: Point<Pixels>, units: (f32, f32)) -> Option<(u32, u32)> {
    clamped(drawn.get()?, at, units)
}

/// A distance of `by` pixels on a screen drawn at `drawn`, in its `units`.
pub fn units_by(drawn: &Drawn, by: Point<Pixels>, units: (f32, f32)) -> Option<(i32, i32)> {
    let bounds = drawn.get()?;
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    (width > 0. && height > 0.).then(|| {
        (
            (f32::from(by.x) / width * units.0).round() as i32,
            (f32::from(by.y) / height * units.1).round() as i32,
        )
    })
}

/// Window points to screen units: the picture fills its bounds exactly, so the scale is
/// the same on both axes and Retina pixels never enter (gpui works in points).
fn clamped(bounds: Bounds<Pixels>, at: Point<Pixels>, units: (f32, f32)) -> Option<(u32, u32)> {
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if width <= 0. || height <= 0. {
        return None;
    }
    let x = f32::from(at.x - bounds.origin.x) / width * units.0;
    let y = f32::from(at.y - bounds.origin.y) / height * units.1;
    // The last unit is the far edge: a click on the border still lands on the screen.
    let hold = |value: f32, most: f32| value.round().clamp(0., (most - 1.).max(0.)) as u32;
    Some((hold(x, units.0), hold(y, units.1)))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    use super::{Drawn, units_at, units_by, units_held};

    fn drawn_at(x: f32, y: f32, width: f32, height: f32) -> Drawn {
        let drawn = Drawn::default();
        drawn.set(Some(Bounds::new(
            point(px(x), px(y)),
            size(px(width), px(height)),
        )));
        drawn
    }

    #[test]
    fn window_points_become_screen_units() {
        // A 402x874 screen drawn at half size, 100,50 from the window's corner.
        let drawn = drawn_at(100., 50., 201., 437.);
        let units = (402., 874.);
        assert_eq!(
            units_at(&drawn, point(px(100.), px(50.)), units),
            Some((0, 0))
        );
        assert_eq!(
            units_at(&drawn, point(px(200.5), px(268.5)), units),
            Some((201, 437))
        );
        assert_eq!(
            units_at(&drawn, point(px(300.9), px(486.9)), units),
            Some((401, 873))
        );
        assert_eq!(units_at(&drawn, point(px(99.), px(60.)), units), None);
        assert_eq!(
            units_held(&drawn, point(px(500.), px(-20.)), units),
            Some((401, 0))
        );
        assert_eq!(
            units_by(&drawn, point(px(0.), px(-10.)), units),
            Some((0, -20))
        );
        assert_eq!(
            units_at(&Drawn::default(), point(px(1.), px(1.)), units),
            None
        );
    }
}
