//! A diagram: a figure in a quiet frame, its title above and its legend below, the picture
//! drawn where the protocol's layout puts each part. Boxes and words are elements, every
//! line and the figures a box cannot draw are one canvas under them (`paint`); texts are
//! measured in the window's own fonts (`measure`). The pointer on a node lights the edges
//! that touch it and quiets the rest; nothing in a diagram can be pressed.

mod measure;
mod paint;

use dusk_protocol::thread::diagram::{
    Diagram, DiagramTone, EdgeStyle, LegendEntry,
    layout::{Anchor, Figure, Owner, TextStyle, layout},
};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::{ui::Type, workspace::Workspace};

/// Space inside the frame.
const PAD: f32 = 16.;

/// The colours a diagram is drawn in, from the theme.
#[derive(Clone, Copy)]
pub struct Palette {
    background: Hsla,
    surface: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    edge: Hsla,
    hair: Hsla,
    accent: Hsla,
    info: Hsla,
    success: Hsla,
    warning: Hsla,
    danger: Hsla,
}

impl Palette {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            background: theme.background,
            surface: theme.muted,
            border: theme.foreground.opacity(0.16),
            text: theme.foreground,
            muted: theme.muted_foreground,
            edge: theme.foreground.opacity(0.42),
            hair: theme.foreground.opacity(0.16),
            accent: theme.primary,
            info: theme.info,
            success: theme.success,
            warning: theme.warning,
            danger: theme.danger,
        }
    }

    /// A node's fill and border in its tone: the tone's colour as the border, a breath of
    /// it as the fill; a muted node is an outline only.
    fn tone(&self, tone: Option<DiagramTone>) -> (Hsla, Hsla) {
        let hue = match tone {
            None => return (self.surface, self.border),
            Some(DiagramTone::Muted) => return (self.background, self.border),
            Some(DiagramTone::Accent) => self.accent,
            Some(DiagramTone::Info) => self.info,
            Some(DiagramTone::Success) => self.success,
            Some(DiagramTone::Warning) => self.warning,
            Some(DiagramTone::Danger) => self.danger,
        };
        (self.background.blend(hue.opacity(0.09)), hue.opacity(0.85))
    }
}

/// Draws the diagram at item `at` of the chat `shown`.
pub fn render(
    ws: &Workspace,
    shown: &str,
    at: usize,
    diagram: &Diagram,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let palette = Palette::of(cx);
    let room = (ws.reading_width - 2. * PAD - 2.).max(160.);
    let laid = layout(diagram, &measure::Fonts::new(cx), room);
    let hover = ws
        .diagram_hover
        .as_ref()
        .filter(|(chat, item, _)| chat == shown && *item == at)
        .map(|(_, _, node)| *node);
    let lit = paint::Lit::of(&laid, hover);

    let mut picture = div()
        .relative()
        .flex_none()
        .w(px(laid.width))
        .h(px(laid.height));
    for group in &laid.groups {
        let r = group.rect;
        picture = picture.child(
            place(r.x, r.y, r.w, r.h)
                .rounded(px(10.))
                .border_1()
                .border_color(palette.hair.opacity(0.7))
                .bg(palette.text.opacity(0.022)),
        );
    }
    picture = picture.child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(paint::canvas_layer(laid.clone(), palette, lit.clone())),
    );
    for node in &laid.nodes {
        let r = node.rect;
        let (fill, border) = palette.tone(node.tone);
        let faded = lit.dims_node(node.node);
        let face = match node.figure {
            Figure::Box | Figure::Pill => {
                let radius = if node.figure == Figure::Pill {
                    r.h / 2.
                } else {
                    8.
                };
                let face = place(r.x, r.y, r.w, r.h)
                    .rounded(px(radius))
                    .border_1()
                    .border_color(border)
                    .bg(fill);
                if node.tone == Some(DiagramTone::Muted) {
                    face.border_dashed()
                } else {
                    face
                }
            }
            // Drawn on the canvas; this box only catches the pointer.
            _ => place(r.x, r.y, r.w, r.h),
        };
        let (chat, index, id) = (shown.to_owned(), at, node.node);
        picture = picture.child(
            face.id(SharedString::from(format!("diagram-{shown}-{at}-{id}")))
                .when(faded, |face| face.opacity(paint::DIM))
                .on_hover(cx.listener(move |ws, hovered: &bool, _, cx| {
                    let key = (chat.clone(), index, id);
                    if *hovered {
                        ws.diagram_hover = Some(key);
                    } else if ws.diagram_hover.as_ref() == Some(&key) {
                        ws.diagram_hover = None;
                    }
                    cx.notify();
                })),
        );
    }
    for text in &laid.texts {
        let style = text.style;
        let color = match (style, text.owner) {
            (TextStyle::Label, _) => palette.text,
            (TextStyle::Edge, Owner::Edge(edge)) if lit.edges.contains(&edge) => palette.text,
            _ => palette.muted,
        };
        let faded = match text.owner {
            Owner::Node(node) => lit.dims_node(node),
            Owner::Edge(edge) => lit.dims_edge(edge),
            Owner::Group(_) => false,
        };
        let line = place(
            text.left() - 1.,
            text.y,
            text.width + 2.,
            style.line_height(),
        )
        .flex()
        .whitespace_nowrap()
        .text_size(px(style.size()))
        .line_height(px(style.line_height()))
        .font_weight(FontWeight(f32::from(style.weight())))
        .text_color(if faded {
            color.opacity(paint::DIM)
        } else {
            color
        });
        let line = match text.anchor {
            Anchor::Start => line.justify_start(),
            Anchor::Middle => line.justify_center(),
            Anchor::End => line.justify_end(),
        };
        picture = picture.child(line.child(SharedString::from(text.text.clone())));
    }

    // A picture wider than the column scrolls inside the frame; the frame never grows.
    // Auto margins centre a narrow picture and fall to nothing for a wide one, so its
    // left end stays in reach.
    let body = div()
        .id(SharedString::from(format!("diagram-scroll-{shown}-{at}")))
        .w_full()
        .min_w_0()
        .flex()
        .overflow_x_scroll()
        .child(picture.mx_auto());
    let mut frame = div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap_3()
        .p(px(PAD))
        .rounded(px(12.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(palette.background);
    if let Some(title) = diagram.title() {
        frame = frame.child(
            div()
                .small()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(SharedString::from(title.to_owned())),
        );
    }
    frame = frame.child(body);
    if !diagram.legend().is_empty() {
        frame = frame.child(legend(diagram.legend(), &palette));
    }
    frame.into_any_element()
}

/// A box at a place of the picture.
fn place(x: f32, y: f32, w: f32, h: f32) -> Div {
    div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h))
}

/// What the tones and line styles mean, in a row under the picture.
fn legend(entries: &[LegendEntry], palette: &Palette) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .children(entries.iter().map(|entry| {
            let key = match (entry.tone, entry.style) {
                (Some(tone), _) => {
                    let (fill, border) = palette.tone(Some(tone));
                    let swatch = div()
                        .size(px(10.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(border)
                        .bg(fill);
                    if tone == DiagramTone::Muted {
                        swatch.border_dashed()
                    } else {
                        swatch
                    }
                }
                (None, style) => line_key(style.unwrap_or_default(), palette),
            };
            div().flex().items_center().gap_1p5().child(key).child(
                div()
                    .meta()
                    .text_color(palette.muted)
                    .child(SharedString::from(entry.label.to_string())),
            )
        }))
}

/// A short line drawn in an edge's style.
fn line_key(style: EdgeStyle, palette: &Palette) -> Div {
    let (color, height) = match style {
        EdgeStyle::Strong => (palette.accent, 2.),
        _ => (palette.edge, 1.5),
    };
    let line = div().w(px(18.)).h(px(height)).bg(color);
    match style {
        EdgeStyle::Dashed => div()
            .w(px(18.))
            .flex()
            .gap(px(3.))
            .children((0..3).map(move |_| div().w(px(4.)).h(px(height)).bg(color))),
        _ => line,
    }
}
