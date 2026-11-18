//! A picture over the whole window. The wheel, a pinch or + and - zoom it, 0 fits it
//! again, a drag moves it; a click beside it or Escape closes it.

use std::io::Cursor;

use gpui_kit::*;

use super::Modal;
use crate::workspace::Workspace;

const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 10.;
/// How much one key press zooms.
const KEY_STEP: f32 = 1.25;
/// The room left around a picture fitted to the window.
const MARGIN: f32 = 32.;

/// How the picture is zoomed and moved, from fitted and centred.
pub struct PictureView {
    pub zoom: f32,
    pub pan: Point<Pixels>,
    /// Where the pointer was at the last move of a drag.
    pub drag: Option<Point<Pixels>>,
}

impl Default for PictureView {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: Point::default(),
            drag: None,
        }
    }
}

/// None for a source whose bytes are not at hand; it is then fitted as a square box.
fn natural_size(image: &ImageSource) -> Option<(u32, u32)> {
    let reader = match image {
        ImageSource::Resource(Resource::Path(path)) => image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions(),
        ImageSource::Image(image) => image::ImageReader::new(Cursor::new(image.bytes()))
            .with_guessed_format()
            .ok()?
            .into_dimensions(),
        _ => return None,
    };
    reader
        .ok()
        .filter(|(width, height)| *width > 0 && *height > 0)
}

pub fn render(
    ws: &Workspace,
    image: &ImageSource,
    size: Option<(u32, u32)>,
    view: &PictureView,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let room = window.viewport_size();
    let (room_w, room_h) = (room.width.as_f32(), room.height.as_f32());
    let (natural_w, natural_h) = size.map_or((room_w, room_h), |(w, h)| (w as f32, h as f32));
    // Fitted means as large as the window allows, but a small picture is not blown up.
    let fit = ((room_w - 2. * MARGIN) / natural_w)
        .min((room_h - 2. * MARGIN) / natural_h)
        .clamp(0.01, 1.);
    let (width, height) = (natural_w * fit * view.zoom, natural_h * fit * view.zoom);
    let left = (room_w - width) / 2. + view.pan.x.as_f32();
    let top = (room_h - height) / 2. + view.pan.y.as_f32();
    let picture = div()
        .id("picture-view")
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .cursor_grab()
        .child(
            img(image.clone())
                .size_full()
                .object_fit(ObjectFit::Contain),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, event: &MouseDownEvent, _, cx| {
                // A press on the picture starts a drag; it must not reach the backdrop.
                cx.stop_propagation();
                if let Some(view) = ws.picture_view() {
                    view.drag = Some(event.position);
                }
            }),
        );
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .bg(black().opacity(0.85))
        // Nothing behind takes the mouse, scrolling included.
        .occlude()
        .track_focus(&ws.modal_focus)
        .on_key_down(cx.listener(|ws, event: &KeyDownEvent, _, cx| {
            match event.keystroke.key.as_str() {
                "+" | "=" => ws.zoom_picture(KEY_STEP, cx),
                "-" => ws.zoom_picture(1. / KEY_STEP, cx),
                "0" => {
                    if let Some(view) = ws.picture_view() {
                        *view = PictureView::default();
                    }
                    cx.notify();
                }
                _ => {}
            }
        }))
        .on_scroll(cx.listener(|ws, event: &ScrollEvent, _, cx| {
            let dy = event.delta.pixel_delta(px(16.)).y.as_f32();
            ws.zoom_picture((dy / 240.).exp(), cx);
        }))
        .on_pinch(cx.listener(|ws, event: &PinchEvent, _, cx| {
            ws.zoom_picture(1. + event.delta, cx);
        }))
        .on_mouse_move(cx.listener(|ws, event: &MouseMoveEvent, _, cx| {
            let Some(view) = ws.picture_view() else {
                return;
            };
            if let Some(last) = view.drag {
                if event.pressed_button != Some(MouseButton::Left) {
                    view.drag = None;
                    return;
                }
                view.pan += event.position - last;
                view.drag = Some(event.position);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _, _, _| {
                if let Some(view) = ws.picture_view() {
                    view.drag = None;
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _, window, cx| ws.close_modal_from_window(window, cx)),
        )
        .child(picture)
}

impl Workspace {
    /// Opens a picture over the window. Its pixel size is read from its header once,
    /// here; the backdrop takes the keys so + and - zoom at once.
    pub fn open_picture(
        &mut self,
        image: ImageSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = natural_size(&image);
        self.modal = Some(Modal::Picture {
            image,
            size,
            view: PictureView::default(),
        });
        self.modal_focus.focus(window, cx);
        cx.notify();
    }

    fn picture_view(&mut self) -> Option<&mut PictureView> {
        match &mut self.modal {
            Some(Modal::Picture { view, .. }) => Some(view),
            _ => None,
        }
    }

    fn zoom_picture(&mut self, by: f32, cx: &mut Context<Self>) {
        if !by.is_finite() {
            return;
        }
        if let Some(view) = self.picture_view() {
            view.zoom = (view.zoom * by).clamp(MIN_ZOOM, MAX_ZOOM);
            cx.notify();
        }
    }
}
