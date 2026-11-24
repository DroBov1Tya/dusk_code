//! The Diagram block: an inline SVG the page builds from the protocol's layout of the
//! diagram, never from markup the model wrote. Every word is an SVG text node; colours come
//! from the theme's variables by class. The pointer on a node lights the edges touching it.

use dusk_protocol::thread::diagram::{
    Diagram, DiagramTone, EdgeStyle, LegendEntry,
    layout::{
        Anchor, Estimate, Figure, GuideKind, HEAD_HALF, Layout, NodeBox, Owner, Seg, TextStyle,
        layout,
    },
};
use leptos::{html, prelude::*};

/// Width a diagram is laid out for before its frame is measured.
const FIRST_WIDTH: f32 = 720.;
/// Space inside the frame, each side.
const PAD: f32 = 16.;

#[component]
pub fn DiagramView(diagram: Diagram) -> impl IntoView {
    let frame = NodeRef::<html::Div>::new();
    let width = RwSignal::new(FIRST_WIDTH);
    // Measured once in the page: the chat column does not change under a block often
    // enough to lay it out again on every resize.
    Effect::new(move |_| {
        if let Some(frame) = frame.get() {
            let inner = frame.client_width() as f32 - 2. * PAD;
            if inner > 0. && (inner - width.get_untracked()).abs() > 1. {
                width.set(inner);
            }
        }
    });
    let hover = RwSignal::new(None::<usize>);
    let title = diagram.title().map(str::to_owned);
    let legend = diagram.legend().to_vec();
    let kind = diagram.kind().name();
    let label = format!(
        "Diagram from the agent, {kind}, {} nodes",
        diagram.nodes().len()
    );
    let text = dusk_protocol::thread::Block::Diagram(diagram.clone()).text();
    view! {
        <div class="shown diagram" role="figure" aria-label=label node_ref=frame>
            {title.map(|title| view! { <div class="diagram-title">{title}</div> })}
            <div class="diagram-body">
                {move || picture(&layout(&diagram, &Estimate, width.get()), hover)}
            </div>
            // The same picture as words, for a screen reader.
            <pre class="sr-only">{text}</pre>
            {(!legend.is_empty()).then(|| view! { <Legend entries=legend /> })}
        </div>
    }
}

fn picture(laid: &Layout, hover: RwSignal<Option<usize>>) -> impl IntoView + use<> {
    let lit_edges =
        move |from: usize, to: usize| hover.get().map(|node| from == node || to == node);
    let edge_ends: Vec<(usize, usize)> = laid.edges.iter().map(|e| (e.from, e.to)).collect();
    let node_lit = move |node: usize| {
        hover.get().map(|hovered| {
            hovered == node
                || edge_ends
                    .iter()
                    .any(|&(a, b)| (a == hovered && b == node) || (b == hovered && a == node))
        })
    };
    let node_lit_text = node_lit.clone();
    let groups = laid
        .groups
        .iter()
        .map(|group| {
            let r = group.rect;
            view! { <rect class="dg-group" x=r.x y=r.y width=r.w height=r.h rx="10" /> }
        })
        .collect_view();
    let guides = laid
        .guides
        .iter()
        .map(|guide| {
            let class = match guide.kind {
                GuideKind::Lifeline => "dg-lifeline",
                GuideKind::Axis => "dg-axis",
                GuideKind::Drop => "dg-drop",
            };
            view! {
                <line class=class x1=guide.from.x y1=guide.from.y x2=guide.to.x y2=guide.to.y />
            }
        })
        .collect_view();
    let edges = laid
        .edges
        .iter()
        .map(|edge| {
            let (from, to) = (edge.from, edge.to);
            let style = match edge.style {
                EdgeStyle::Solid => "dg-edge",
                EdgeStyle::Dashed => "dg-edge dashed",
                EdgeStyle::Strong => "dg-edge strong",
            };
            let head = edge.head.map(|head| {
                let [a, b, c] = head.corners(HEAD_HALF);
                format!("{},{} {},{} {},{}", a.x, a.y, b.x, b.y, c.x, c.y)
            });
            let mask = edge.label;
            view! {
                <g class=style
                    class:lit=move || lit_edges(from, to) == Some(true)
                    class:dim=move || lit_edges(from, to) == Some(false)>
                    <path d=path(&edge.segs) />
                    {head.map(|points| view! { <polygon points=points /> })}
                    {mask.map(|r| view! {
                        <rect class="dg-mask" x=r.x y=r.y width=r.w height=r.h rx="4" />
                    })}
                </g>
            }
        })
        .collect_view();
    let nodes = laid
        .nodes
        .iter()
        .map(|node| {
            let id = node.node;
            let lit = node_lit.clone();
            view! {
                <g class=format!("dg-node {}", tone_class(node.tone))
                    class:dim=move || lit(id) == Some(false)
                    on:mouseenter=move |_| hover.set(Some(id))
                    on:mouseleave=move |_| hover.update(|h| if *h == Some(id) { *h = None })>
                    {figure(node)}
                </g>
            }
        })
        .collect_view();
    let texts = laid
        .texts
        .iter()
        .map(|text| {
            let anchor = match text.anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let class = match text.style {
                TextStyle::Label => "dg-label",
                TextStyle::Note => "dg-note",
                TextStyle::Edge => "dg-edge-label",
                TextStyle::Group => "dg-group-label",
                TextStyle::Axis => "dg-axis-label",
            };
            let owner = text.owner;
            let lit = node_lit_text.clone();
            let dim = move || match owner {
                Owner::Node(node) => lit(node) == Some(false),
                Owner::Edge(_) | Owner::Group(_) => false,
            };
            view! {
                <text class=class class:dim=dim x=text.x
                    y=text.y + text.style.line_height() / 2.
                    font-size=text.style.size() font-weight=text.style.weight()
                    text-anchor=anchor dominant-baseline="central">
                    {text.text.clone()}
                </text>
            }
        })
        .collect_view();
    let (w, h) = (laid.width, laid.height);
    view! {
        <svg class="dg" width=w height=h viewBox=format!("0 0 {w} {h}") aria-hidden="true">
            {groups}{guides}{edges}{nodes}{texts}
        </svg>
    }
}

fn tone_class(tone: Option<DiagramTone>) -> &'static str {
    match tone {
        None => "plain",
        Some(DiagramTone::Accent) => "accent",
        Some(DiagramTone::Info) => "info",
        Some(DiagramTone::Success) => "good",
        Some(DiagramTone::Warning) => "warn",
        Some(DiagramTone::Danger) => "bad",
        Some(DiagramTone::Muted) => "muted",
    }
}

/// An SVG path of the layout's pieces of line.
fn path(segs: &[Seg]) -> String {
    let mut d = String::new();
    for (at, seg) in segs.iter().enumerate() {
        if at == 0 {
            d.push_str(&format!("M{} {}", seg.start().x, seg.start().y));
        }
        match *seg {
            Seg::Line(_, b) => d.push_str(&format!(" L{} {}", b.x, b.y)),
            Seg::Cubic(_, b, c, e) => d.push_str(&format!(
                " C{} {} {} {} {} {}",
                b.x, b.y, c.x, c.y, e.x, e.y
            )),
        }
    }
    d
}

fn figure(node: &NodeBox) -> AnyView {
    let r = node.rect;
    let c = r.center();
    match node.figure {
        Figure::Box => view! { <rect x=r.x y=r.y width=r.w height=r.h rx="8" /> }.into_any(),
        Figure::Pill => view! { <rect x=r.x y=r.y width=r.w height=r.h rx=r.h / 2. /> }.into_any(),
        Figure::Diamond => {
            let points = format!(
                "{},{} {},{} {},{} {},{}",
                c.x,
                r.y,
                r.right(),
                c.y,
                c.x,
                r.bottom(),
                r.x,
                c.y
            );
            view! { <polygon points=points stroke-linejoin="round" /> }.into_any()
        }
        Figure::Store => {
            let ry = 5.;
            let (top, bottom, rx) = (r.y + ry, r.bottom() - ry, r.w / 2.);
            let body = format!(
                "M{x} {top} A{rx} {ry} 0 0 1 {xr} {top} L{xr} {bottom} A{rx} {ry} 0 0 1 {x} {bottom} Z",
                x = r.x,
                xr = r.right()
            );
            let lid = format!(
                "M{x} {top} A{rx} {ry} 0 0 0 {xr} {top}",
                x = r.x,
                xr = r.right()
            );
            view! { <path d=body /><path class="dg-lid" d=lid /> }.into_any()
        }
        Figure::Dot => view! { <circle class="dg-mark" cx=c.x cy=c.y r=r.w / 2. /> }.into_any(),
        Figure::Ring => view! {
            <circle class="dg-ring" cx=c.x cy=c.y r=r.w / 2. - 0.75 />
            <circle class="dg-mark" cx=c.x cy=c.y r=r.w / 2. - 4. />
        }
        .into_any(),
        Figure::Event => view! { <circle class="dg-event" cx=c.x cy=c.y r=r.w / 2. /> }.into_any(),
    }
}

#[component]
fn Legend(entries: Vec<LegendEntry>) -> impl IntoView {
    view! {
        <ul class="diagram-legend">
            {entries.into_iter().map(|entry| {
                let key = match (entry.tone, entry.style) {
                    (Some(tone), _) => format!("swatch {}", tone_class(Some(tone))),
                    (None, style) => format!("stroke {}", style.unwrap_or_default().name()),
                };
                view! {
                    <li><span class=key aria-hidden="true"></span>{entry.label.to_string()}</li>
                }
            }).collect_view()}
        </ul>
    }
}
