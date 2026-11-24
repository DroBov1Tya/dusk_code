//! A diagram is model output: every way a call can be wrong is refused with words that
//! say what to fix, and nothing out of its caps is ever built.

use serde_json::{Value, json};

use super::*;
use crate::thread::Block;

fn read(diagram: Value) -> Result<Diagram, String> {
    serde_json::from_value::<Diagram>(diagram).map_err(|error| error.to_string())
}

fn refused(diagram: Value, said: &str) {
    let error = read(diagram.clone()).expect_err(&format!("{diagram}"));
    assert!(error.contains(said), "{said:?} not in {error:?}");
}

fn nodes(count: usize) -> Vec<Value> {
    (0..count)
        .map(|n| json!({ "id": format!("n{n}") }))
        .collect()
}

#[test]
fn a_flow_reads_and_writes_back_the_same() {
    let diagram = read(json!({
        "kind": "flow", "title": "Import",
        "nodes": [
            { "id": "in", "label": "Upload", "note": "8 MiB cap", "group": "edge" },
            { "id": "ok", "label": "Valid?", "shape": "diamond", "tone": "accent" },
            { "id": "db", "shape": "store" },
        ],
        "edges": [
            { "from": "in", "to": "ok" },
            { "from": "ok", "to": "db", "label": "yes", "style": "strong" },
        ],
        "groups": [{ "id": "edge", "label": "Edge" }],
        "legend": [{ "label": "changed", "tone": "accent" }],
    }))
    .unwrap();
    let again: Diagram = serde_json::from_value(serde_json::to_value(&diagram).unwrap()).unwrap();
    assert_eq!(again, diagram);
    assert_eq!(diagram.nodes()[2].label(), "db");
    let text = Block::Diagram(diagram).text();
    assert_eq!(
        text,
        "Diagram:\nflow: Import\nUpload (8 MiB cap) [Edge]\n  -> Valid?\nValid?\n  -> db: yes\n\
         db\nlegend: accent = changed"
    );
}

#[test]
fn counts_past_the_caps_are_refused() {
    refused(
        json!({ "kind": "flow", "nodes": nodes(MAX_NODES + 1) }),
        "1 to 24 nodes",
    );
    refused(json!({ "kind": "flow", "nodes": [] }), "1 to 24 nodes");
    refused(
        json!({ "kind": "sequence", "nodes": nodes(MAX_PARTICIPANTS + 1) }),
        "2 to 6 nodes",
    );
    refused(
        json!({ "kind": "timeline", "nodes": nodes(MAX_EVENTS + 1) }),
        "2 to 12 nodes",
    );
    let edges: Vec<Value> = (0..=MAX_EDGES)
        .map(|_| json!({ "from": "n0", "to": "n1" }))
        .collect();
    refused(
        json!({ "kind": "flow", "nodes": nodes(2), "edges": edges }),
        "at most 40 edges",
    );
    let groups: Vec<Value> = (0..=MAX_GROUPS)
        .map(|g| json!({ "id": format!("g{g}"), "label": "x" }))
        .collect();
    refused(
        json!({ "kind": "architecture", "nodes": nodes(2), "groups": groups }),
        "at most 4 groups",
    );
    let toned: Vec<Value> = (0..=MAX_TONED)
        .map(|n| json!({ "id": format!("n{n}"), "tone": "danger" }))
        .collect();
    refused(json!({ "kind": "flow", "nodes": toned }), "at most 3 nodes");
}

#[test]
fn texts_and_ids_are_checked() {
    let long = "x".repeat(NODE_LABEL_CHARS + 1);
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "label": long }] }),
        "over 40 characters",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "label": "two\nlines" }] }),
        "one line",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a b" }] }),
        "may only have letters",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "x".repeat(ID_CHARS + 1) }] }),
        "1 to 24 characters",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "svg": "<svg/>" }] }),
        "unknown field",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "tone": "#ff0000" }] }),
        "unknown variant",
    );
}

#[test]
fn every_id_named_must_be_given_once() {
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a" }, { "id": "a" }] }),
        "used twice",
    );
    refused(
        json!({ "kind": "flow", "nodes": nodes(1), "edges": [{ "from": "n0", "to": "zz" }] }),
        "\"zz\", which is not given",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "group": "g" }] }),
        "which is not given",
    );
    refused(
        json!({ "kind": "flow", "nodes": nodes(1), "groups": [{ "id": "g", "label": "G" }] }),
        "has no nodes",
    );
}

#[test]
fn each_kind_keeps_to_its_own_fields() {
    refused(
        json!({ "kind": "flow", "nodes": nodes(1), "edges": [{ "from": "n0", "to": "n0" }] }),
        "only state and sequence",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "shape": "start" }] }),
        "for a state diagram",
    );
    refused(
        json!({ "kind": "timeline", "nodes": nodes(2), "edges": [{ "from": "n0", "to": "n1" }] }),
        "at most 0 edges",
    );
    refused(
        json!({ "kind": "flow", "nodes": [{ "id": "a", "when": "May" }] }),
        "when is for a timeline",
    );
    refused(
        json!({ "kind": "sequence", "direction": "right", "nodes": nodes(2) }),
        "direction is for",
    );
    refused(
        json!({ "kind": "tree", "nodes": [{ "id": "a", "group": "g" }],
                "groups": [{ "id": "g", "label": "G" }] }),
        "groups are for",
    );
    refused(
        json!({ "kind": "flow", "nodes": nodes(1), "legend": [{ "label": "x" }] }),
        "exactly one of tone and style",
    );
}

#[test]
fn a_tree_has_one_parent_a_node_and_no_circle() {
    let tree = |edges: Value| json!({ "kind": "tree", "nodes": nodes(3), "edges": edges });
    assert!(
        read(tree(
            json!([{ "from": "n0", "to": "n1" }, { "from": "n0", "to": "n2" }])
        ))
        .is_ok()
    );
    refused(
        tree(json!([{ "from": "n0", "to": "n2" }, { "from": "n1", "to": "n2" }])),
        "two parents",
    );
    refused(
        tree(
            json!([{ "from": "n0", "to": "n1" }, { "from": "n1", "to": "n2" }, { "from": "n2", "to": "n0" }]),
        ),
        "circle",
    );
}

#[test]
fn a_sequence_and_a_timeline_read_as_text() {
    let sequence = read(json!({
        "kind": "sequence",
        "nodes": [{ "id": "c", "label": "Client" }, { "id": "s", "label": "Server" }],
        "edges": [
            { "from": "c", "to": "s", "label": "GET /" },
            { "from": "s", "to": "c", "label": "200", "style": "dashed" },
        ],
    }))
    .unwrap();
    assert_eq!(
        Block::Diagram(sequence).text(),
        "Diagram:\nsequence\nparticipants: Client, Server\n1. Client -> Server: GET /\n\
         2. Server --> Client: 200"
    );
    let timeline = read(json!({
        "kind": "timeline",
        "nodes": [
            { "id": "a", "label": "Beta", "when": "May" },
            { "id": "b", "label": "1.0", "when": "June", "note": "stable" },
        ],
    }))
    .unwrap();
    assert_eq!(
        Block::Diagram(timeline).text(),
        "Diagram:\ntimeline\nMay: Beta\nJune: 1.0 - stable"
    );
}
