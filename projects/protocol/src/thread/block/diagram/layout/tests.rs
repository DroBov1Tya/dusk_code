//! The layout is the same for the same diagram, keeps its parts apart and inside the
//! picture, and never fails on any diagram within the caps.

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::thread::diagram::{MAX_EDGES, MAX_NODES};

fn diagram(value: Value) -> Diagram {
    serde_json::from_value(value).expect("a diagram within caps")
}

fn architecture() -> Diagram {
    diagram(json!({
        "kind": "architecture", "title": "Request path",
        "nodes": [
            { "id": "web", "label": "Web client", "note": "wasm", "group": "clients" },
            { "id": "tui", "label": "Terminal", "group": "clients" },
            { "id": "server", "label": "Server", "note": "axum, TLS" },
            { "id": "core", "label": "Core", "note": "threads, tools", "tone": "accent", "group": "engine" },
            { "id": "db", "label": "SQLite", "shape": "store", "group": "engine" },
            { "id": "llm", "label": "Provider", "tone": "muted" },
        ],
        "edges": [
            { "from": "web", "to": "server", "label": "HTTP" },
            { "from": "tui", "to": "core", "label": "in process" },
            { "from": "server", "to": "core" },
            { "from": "core", "to": "db", "label": "WAL" },
            { "from": "core", "to": "llm", "label": "stream", "style": "dashed" },
        ],
        "groups": [
            { "id": "clients", "label": "Clients" },
            { "id": "engine", "label": "Engine" },
        ],
    }))
}

fn state() -> Diagram {
    diagram(json!({
        "kind": "state", "direction": "right",
        "nodes": [
            { "id": "s", "shape": "start" }, { "id": "idle" }, { "id": "running" },
            { "id": "waiting" }, { "id": "failed", "tone": "danger" }, { "id": "e", "shape": "end" },
        ],
        "edges": [
            { "from": "s", "to": "idle" },
            { "from": "idle", "to": "running", "label": "send" },
            { "from": "running", "to": "waiting", "label": "rate limited" },
            { "from": "waiting", "to": "running", "label": "wake" },
            { "from": "running", "to": "running", "label": "step" },
            { "from": "running", "to": "failed", "label": "error" },
            { "from": "running", "to": "idle", "label": "done" },
            { "from": "failed", "to": "e" },
        ],
    }))
}

/// Everything a picture must hold to: finite numbers, parts inside it, nodes apart,
/// labels off the nodes and off each other, groups around their own nodes only.
fn check(diagram: &Diagram, layout: &Layout) {
    let picture = Rect::new(0., 0., layout.width, layout.height);
    assert!(layout.width.is_finite() && layout.height.is_finite());
    assert_eq!(layout.nodes.len(), diagram.nodes().len());
    for node in &layout.nodes {
        assert!(
            node.rect.x.is_finite() && node.rect.y.is_finite(),
            "{node:?}"
        );
        assert!(
            picture.contains(node.rect.center()),
            "{node:?} outside {picture:?}"
        );
    }
    for (at, a) in layout.nodes.iter().enumerate() {
        for b in &layout.nodes[at + 1..] {
            assert!(!a.rect.overlaps(&b.rect), "{a:?} overlaps {b:?}");
        }
    }
    let labels: Vec<Rect> = layout.edges.iter().filter_map(|edge| edge.label).collect();
    for (at, label) in labels.iter().enumerate() {
        for node in &layout.nodes {
            assert!(!label.overlaps(&node.rect), "label {label:?} on {node:?}");
        }
        for other in &labels[at + 1..] {
            assert!(!label.overlaps(other), "label {label:?} on label {other:?}");
        }
    }
    for edge in &layout.edges {
        for point in polyline(&edge.segs) {
            assert!(point.x.is_finite() && point.y.is_finite(), "{edge:?}");
        }
    }
    for group in &layout.groups {
        for node in &layout.nodes {
            let inside = diagram.group_of(node.node) == Some(group.group);
            if !inside {
                assert!(!group.rect.overlaps(&node.rect), "{node:?} in {group:?}");
            }
        }
    }
}

#[test]
fn the_same_diagram_is_laid_out_the_same() {
    for diagram in [architecture(), state()] {
        let first = layout(&diagram, &Estimate, 640.);
        assert_eq!(first, layout(&diagram, &Estimate, 640.));
        check(&diagram, &first);
    }
}

#[test]
fn a_wide_diagram_turns_down_and_a_narrow_tree_becomes_an_outline() {
    let state = state();
    let wide = layout(&state, &Estimate, 2000.);
    let narrow = layout(&state, &Estimate, 420.);
    assert!(wide.width > wide.height, "{} x {}", wide.width, wide.height);
    assert!(narrow.width < wide.width);
    check(&state, &narrow);

    let leaves: Vec<Value> = (0..10)
        .map(|n| json!({ "id": format!("leaf{n}"), "label": format!("Module number {n}") }))
        .collect();
    let mut nodes = vec![json!({ "id": "root", "label": "src" })];
    nodes.extend(leaves);
    let edges: Vec<Value> = (0..10)
        .map(|n| json!({ "from": "root", "to": format!("leaf{n}") }))
        .collect();
    let tree = diagram(json!({ "kind": "tree", "nodes": nodes, "edges": edges }));
    let outline = layout(&tree, &Estimate, 360.);
    assert!(outline.width <= 360., "{}", outline.width);
    check(&tree, &outline);
}

#[test]
fn a_timeline_stands_when_it_does_not_fit_across() {
    let events: Vec<Value> = (0..8)
        .map(|n| json!({ "id": format!("e{n}"), "label": format!("Release candidate {n}"), "when": format!("Week {n}") }))
        .collect();
    let timeline = diagram(json!({ "kind": "timeline", "nodes": events }));
    let across = layout(&timeline, &Estimate, 4000.);
    let down = layout(&timeline, &Estimate, 360.);
    assert!(across.width > across.height);
    assert!(down.width <= 360., "{}", down.width);
    check(&timeline, &across);
    check(&timeline, &down);
}

#[test]
fn a_sequence_gives_each_message_its_room() {
    let sequence = diagram(json!({
        "kind": "sequence",
        "nodes": [{ "id": "c", "label": "Client" }, { "id": "a", "label": "API" }, { "id": "d", "label": "Database" }],
        "edges": [
            { "from": "c", "to": "a", "label": "POST /threads with a long body" },
            { "from": "a", "to": "a", "label": "check the token" },
            { "from": "a", "to": "d", "label": "insert" },
            { "from": "d", "to": "a", "style": "dashed" },
            { "from": "a", "to": "c", "label": "201 Created", "style": "dashed" },
        ],
    }));
    let laid = layout(&sequence, &Estimate, 800.);
    check(&sequence, &laid);
    // The first label lies between the first two lifelines, not over the third.
    let first = laid.edges[0].label.unwrap();
    let lifelines: Vec<f32> = laid.guides.iter().map(|g| g.from.x).collect();
    assert!(
        first.x >= lifelines[0] && first.right() <= lifelines[1],
        "{first:?} {lifelines:?}"
    );
}

fn any_diagram() -> impl Strategy<Value = Value> {
    let kinds = prop::sample::select(vec!["flow", "architecture", "state", "tree"]);
    (kinds, 1usize..=MAX_NODES, prop::collection::vec((0usize..64, 0usize..64, any::<bool>(), 0u8..3), 0..MAX_EDGES), any::<bool>(), 0usize..3)
        .prop_map(|(kind, count, raw, right, groups)| {
            let nodes: Vec<Value> = (0..count)
                .map(|n| {
                    let mut node = json!({ "id": format!("n{n}"), "label": format!("Node {} {}", n, "word ".repeat(n % 5)) });
                    if groups > 0 && n % 3 == 0 && kind != "tree" {
                        node["group"] = json!(format!("g{}", n % groups));
                    }
                    if n % 7 == 3 && kind != "tree" {
                        node["shape"] = json!("diamond");
                    }
                    node
                })
                .collect();
            let mut edges = Vec::new();
            let mut parent = vec![false; count];
            for (a, b, labelled, style) in raw {
                let (from, to) = (a % count, b % count);
                if kind == "tree" {
                    // Parent before child, one parent each: always a tree.
                    if from >= to || parent[to] {
                        continue;
                    }
                    parent[to] = true;
                } else if from == to && kind != "state" {
                    continue;
                }
                let mut edge = json!({ "from": format!("n{from}"), "to": format!("n{to}") });
                if labelled {
                    edge["label"] = json!(format!("on {from} to {to}"));
                }
                edge["style"] = json!(["solid", "dashed", "strong"][style as usize]);
                edges.push(edge);
            }
            let used: Vec<usize> = (0..groups).filter(|g| (0..count).any(|n| n % 3 == 0 && n % groups == *g)).collect();
            let groups: Vec<Value> = if kind == "tree" { Vec::new() } else {
                used.iter().map(|g| json!({ "id": format!("g{g}"), "label": format!("Group {g}") })).collect()
            };
            json!({
                "kind": kind,
                "direction": if right { "right" } else { "down" },
                "nodes": nodes, "edges": edges, "groups": groups,
            })
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn any_diagram_within_the_caps_is_laid_out_cleanly(value in any_diagram(), width in 280f32..1200.) {
        let diagram = diagram(value);
        let first = layout(&diagram, &Estimate, width);
        check(&diagram, &first);
        prop_assert_eq!(first, layout(&diagram, &Estimate, width));
    }
}
