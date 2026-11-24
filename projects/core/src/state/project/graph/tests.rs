use super::*;

const P: &str = "/work/p";
const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

async fn state() -> State {
    let state = State::in_memory().await.unwrap();
    state.create_project("p", "p", "", P).await.unwrap();
    state
}

fn file(path: &str, lines: u64, hash: &str) -> WalkedFile {
    WalkedFile {
        path: path.into(),
        lines,
        bytes: lines * 10,
        hash: hash.into(),
        modified_ms: 1,
    }
}

fn node(name: &str, kind: NodeKind) -> NewNode {
    NewNode {
        name: name.into(),
        kind,
        line: Some(1),
        end_line: Some(2),
        summary: format!("{name} does it"),
    }
}

fn edge(from: Option<&str>, to: &str, kind: EdgeKind) -> NewEdge {
    NewEdge {
        from: from.map(str::to_owned),
        to: to.into(),
        kind,
    }
}

const AGENT: Author<'static> = Author {
    thread: Some("t1"),
    by_user: false,
};
const USER: Author<'static> = Author {
    thread: Some("t1"),
    by_user: true,
};

#[tokio::test]
async fn a_walk_is_stored_and_facts_go_stale_when_the_file_changes() {
    let state = state().await;
    state
        .graph_update(P, &[file("src/a.rs", 10, H1), file("src/b.rs", 5, H1)], &[])
        .await
        .unwrap();
    let record = Record {
        summary: Some("Parses input.".into()),
        nodes: vec![node("parse", NodeKind::Function)],
        edges: vec![edge(Some("parse"), "src/b.rs", EdgeKind::Imports)],
    };
    state
        .graph_record(P, "src/a.rs", H1, &record, AGENT)
        .await
        .unwrap();
    let map = state.graph_map(P, Some("src")).await.unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!((map[0].lines, map[0].stale), (10, false));
    assert_eq!(map[0].summary, "Parses input.");

    // The file changed: its facts are kept and marked, never dropped by a walk.
    state
        .graph_update(P, &[file("src/a.rs", 12, H2)], &[])
        .await
        .unwrap();
    let map = state.graph_map(P, None).await.unwrap();
    assert!(map[0].stale);
    let nodes = state.graph_file_nodes(P, "src/a.rs", 10).await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert!(nodes[0].stale);
    let edges = state
        .graph_edges_from(P, "src/a.rs", None, 10)
        .await
        .unwrap();
    assert!(edges[0].stale);

    // A file that is gone takes its facts along.
    state
        .graph_update(P, &[], &["src/a.rs".into()])
        .await
        .unwrap();
    assert!(state.graph_file(P, "src/a.rs").await.unwrap().is_none());
    let edges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_edges")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(edges, 0);
}

#[tokio::test]
async fn a_new_record_replaces_stale_agent_facts_and_keeps_the_users() {
    let state = state().await;
    state
        .graph_update(P, &[file("a.rs", 3, H1)], &[])
        .await
        .unwrap();
    let first = Record {
        summary: None,
        nodes: vec![
            node("old", NodeKind::Function),
            node("kept", NodeKind::Type),
        ],
        edges: vec![],
    };
    state
        .graph_record(P, "a.rs", H1, &first, AGENT)
        .await
        .unwrap();
    // The user says `kept` does something else; that is fresh at H1.
    let fixed = state
        .graph_fix(
            P,
            "a.rs",
            H1,
            &FixTarget::Node {
                name: "kept".into(),
                kind: None,
            },
            Some("what the user said"),
            USER,
        )
        .await
        .unwrap();
    assert!(fixed);
    // An agent recording again at H1 does not overwrite the user's words.
    let again = Record {
        nodes: vec![node("kept", NodeKind::Type)],
        ..Record::default()
    };
    let done = state
        .graph_record(P, "a.rs", H1, &again, AGENT)
        .await
        .unwrap();
    assert_eq!(done.kept, 1);
    let nodes = state.graph_file_nodes(P, "a.rs", 10).await.unwrap();
    let kept = nodes.iter().find(|node| node.name == "kept").unwrap();
    assert_eq!(kept.summary, "what the user said");
    assert!(kept.by_user);

    // The file changes; a record of it replaces the agent's stale node only.
    state
        .graph_update(P, &[file("a.rs", 4, H2)], &[])
        .await
        .unwrap();
    let after = Record {
        nodes: vec![node("new", NodeKind::Function)],
        ..Record::default()
    };
    let done = state
        .graph_record(P, "a.rs", H2, &after, AGENT)
        .await
        .unwrap();
    assert_eq!(done.replaced, 1);
    let mut names: Vec<String> = state
        .graph_file_nodes(P, "a.rs", 10)
        .await
        .unwrap()
        .into_iter()
        .map(|node| node.name)
        .collect();
    names.sort();
    assert_eq!(names, ["kept", "new"]);
}

#[tokio::test]
async fn untrusted_records_are_refused_whole() {
    let state = state().await;
    state
        .graph_update(P, &[file("a.rs", 3, H1), file("big.bin", 0, "")], &[])
        .await
        .unwrap();
    let long = "x".repeat(401);
    let bad = [
        Record {
            summary: Some(long),
            ..Record::default()
        },
        Record {
            summary: Some("a\u{1b}[31m".into()),
            ..Record::default()
        },
        Record {
            nodes: vec![NewNode {
                name: " ".into(),
                ..node("x", NodeKind::Other)
            }],
            ..Record::default()
        },
        Record {
            nodes: vec![NewNode {
                name: "n".repeat(129),
                ..node("x", NodeKind::Other)
            }],
            ..Record::default()
        },
        Record {
            nodes: vec![NewNode {
                line: Some(9),
                end_line: Some(3),
                ..node("x", NodeKind::Other)
            }],
            ..Record::default()
        },
        Record {
            nodes: vec![NewNode {
                line: Some(0),
                ..node("x", NodeKind::Other)
            }],
            ..Record::default()
        },
        // An edge must start at a node of this file.
        Record {
            edges: vec![edge(Some("nowhere"), "b", EdgeKind::Calls)],
            ..Record::default()
        },
    ];
    for record in &bad {
        assert!(
            state
                .graph_record(P, "a.rs", H1, record, AGENT)
                .await
                .is_err(),
            "{record:?}"
        );
    }
    // Nothing of a refused record was written, a good one in the same call or not.
    let mixed = Record {
        summary: Some("fine".into()),
        nodes: vec![node("ok", NodeKind::Function)],
        edges: vec![edge(Some("missing"), "b", EdgeKind::Calls)],
    };
    assert!(
        state
            .graph_record(P, "a.rs", H1, &mixed, AGENT)
            .await
            .is_err()
    );
    assert!(
        state
            .graph_file_nodes(P, "a.rs", 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        state.graph_file(P, "a.rs").await.unwrap().unwrap().summary,
        ""
    );
    // A hash that is not the stored one, a file not walked, a file not hashed.
    let record = Record {
        summary: Some("s".into()),
        ..Record::default()
    };
    assert!(
        state
            .graph_record(P, "a.rs", H2, &record, AGENT)
            .await
            .is_err()
    );
    assert!(
        state
            .graph_record(P, "none.rs", H1, &record, AGENT)
            .await
            .is_err()
    );
    assert!(
        state
            .graph_record(P, "big.bin", "", &record, AGENT)
            .await
            .is_err()
    );
    // Another project's file is not this one's.
    assert!(
        state
            .graph_record("/work/q", "a.rs", H1, &record, AGENT)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_file_holds_a_bounded_number_of_facts() {
    let state = state().await;
    state
        .graph_update(P, &[file("a.rs", 3, H1)], &[])
        .await
        .unwrap();
    let too_many = Record {
        nodes: (0..=facts::MAX_FILE_NODES)
            .map(|n| node(&format!("n{n}"), NodeKind::Function))
            .collect(),
        ..Record::default()
    };
    assert!(
        state
            .graph_record(P, "a.rs", H1, &too_many, AGENT)
            .await
            .is_err()
    );
    assert!(
        state
            .graph_file_nodes(P, "a.rs", 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn edges_resolve_by_name_path_and_path_name_and_impact_goes_two_hops() {
    let state = state().await;
    let files = ["core.rs", "api.rs", "web.rs", "tests.rs", "far.rs"].map(|p| file(p, 1, H1));
    state.graph_update(P, &files, &[]).await.unwrap();
    let core = Record {
        nodes: vec![node("Engine", NodeKind::Type)],
        ..Record::default()
    };
    state
        .graph_record(P, "core.rs", H1, &core, AGENT)
        .await
        .unwrap();
    let api = Record {
        nodes: vec![node("serve", NodeKind::Function)],
        edges: vec![edge(Some("serve"), "Engine", EdgeKind::Uses)],
        ..Record::default()
    };
    state
        .graph_record(P, "api.rs", H1, &api, AGENT)
        .await
        .unwrap();
    let tests = Record {
        edges: vec![edge(None, "core.rs#Engine", EdgeKind::Covers)],
        ..Record::default()
    };
    state
        .graph_record(P, "tests.rs", H1, &tests, AGENT)
        .await
        .unwrap();
    let web = Record {
        edges: vec![edge(None, "api.rs", EdgeKind::Imports)],
        ..Record::default()
    };
    state
        .graph_record(P, "web.rs", H1, &web, AGENT)
        .await
        .unwrap();
    let far = Record {
        edges: vec![edge(None, "web.rs", EdgeKind::Imports)],
        ..Record::default()
    };
    state
        .graph_record(P, "far.rs", H1, &far, AGENT)
        .await
        .unwrap();

    let into = state.graph_edges_into_file(P, "core.rs", 10).await.unwrap();
    let from: Vec<&str> = into.iter().map(|edge| edge.path.as_str()).collect();
    assert_eq!(from, ["api.rs", "tests.rs"]);
    let into = state
        .graph_edges_into_node(P, "core.rs", "Engine", 10)
        .await
        .unwrap();
    assert_eq!(into.len(), 2);
    assert_eq!(into[0].from.as_deref(), Some("serve"));

    let impact = state
        .graph_impact(P, &["core.rs".into()], 10)
        .await
        .unwrap();
    let got: Vec<(&str, u8)> = impact
        .iter()
        .map(|file| (file.path.as_str(), file.hops))
        .collect();
    assert_eq!(got, [("api.rs", 1), ("tests.rs", 1), ("web.rs", 2)]);
    let capped = state.graph_impact(P, &["core.rs".into()], 1).await.unwrap();
    assert_eq!(capped.len(), 1);

    let found = state.graph_find(P, "engine", 10).await.unwrap();
    assert!(matches!(&found[..], [Found::Node(node)] if node.name == "Engine"));
    // `%` and `_` are text, not wildcards.
    assert!(state.graph_find(P, "%", 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn fix_edits_or_deletes_one_fact() {
    let state = state().await;
    state
        .graph_update(P, &[file("a.rs", 3, H1)], &[])
        .await
        .unwrap();
    let record = Record {
        summary: Some("wrong".into()),
        nodes: vec![node("f", NodeKind::Function), node("f", NodeKind::Test)],
        edges: vec![edge(None, "b.rs", EdgeKind::Imports)],
    };
    state
        .graph_record(P, "a.rs", H1, &record, AGENT)
        .await
        .unwrap();
    let summary = FixTarget::Summary;
    assert!(
        state
            .graph_fix(P, "a.rs", H1, &summary, Some("right"), USER)
            .await
            .unwrap()
    );
    let file = state.graph_file(P, "a.rs").await.unwrap().unwrap();
    assert_eq!((file.summary.as_str(), file.by_user), ("right", true));
    // Two nodes share the name: the kind picks one.
    let both = FixTarget::Node {
        name: "f".into(),
        kind: None,
    };
    assert!(
        state
            .graph_fix(P, "a.rs", H1, &both, None, USER)
            .await
            .is_err()
    );
    let test = FixTarget::Node {
        name: "f".into(),
        kind: Some(NodeKind::Test),
    };
    assert!(
        state
            .graph_fix(P, "a.rs", H1, &test, None, USER)
            .await
            .unwrap()
    );
    assert_eq!(
        state.graph_file_nodes(P, "a.rs", 10).await.unwrap().len(),
        1
    );
    let edge = FixTarget::Edge {
        from: None,
        to: "b.rs".into(),
        kind: EdgeKind::Imports,
    };
    assert!(
        state
            .graph_fix(P, "a.rs", H1, &edge, Some("x"), USER)
            .await
            .is_err()
    );
    assert!(
        state
            .graph_fix(P, "a.rs", H1, &edge, None, USER)
            .await
            .unwrap()
    );
    assert!(
        !state
            .graph_fix(P, "a.rs", H1, &edge, None, USER)
            .await
            .unwrap()
    );
    assert!(
        !state
            .graph_fix(P, "zz.rs", H1, &summary, None, USER)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn a_project_moved_to_another_folder_forgets_its_graph() {
    let state = state().await;
    state
        .graph_update(P, &[file("a.rs", 3, H1)], &[])
        .await
        .unwrap();
    assert!(
        state
            .update_project("p", "p", "", "/work/other")
            .await
            .unwrap()
    );
    assert!(
        state
            .graph_map("/work/other", None)
            .await
            .unwrap()
            .is_empty()
    );
    state
        .graph_update("/work/other", &[file("a.rs", 3, H1)], &[])
        .await
        .unwrap();
    state.remove_project("p").await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_files")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
