use super::*;
use crate::testing::temp_state;

const P: &str = "/work/p";
const Q: &str = "/work/q";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A state with two saved projects, `P` and `Q`.
async fn state(name: &str) -> State {
    let state = temp_state(name).await;
    state.create_project("p", "P", "", P).await.unwrap();
    state.create_project("q", "Q", "", Q).await.unwrap();
    state
}

fn file(path: &str, symbols: Vec<NewSymbol>, refs: Vec<NewRef>) -> IndexedFile {
    IndexedFile {
        path: path.to_owned(),
        language: Language::Rust,
        hash: HASH.to_owned(),
        stamp: Stamp {
            size: 10,
            modified_ms: 1,
        },
        module: path.trim_end_matches(".rs").replace('/', "::"),
        is_test: path.contains("tests"),
        skipped: None,
        symbols,
        refs,
    }
}

fn symbol(name: &str, kind: SymbolKind, line: u32) -> NewSymbol {
    NewSymbol {
        name: name.to_owned(),
        parent: None,
        kind,
        line,
        end_line: line + 2,
    }
}

fn defines(name: &str) -> NewSymbol {
    symbol(name, SymbolKind::Type, 1)
}

fn test_fn(name: &str) -> NewSymbol {
    symbol(name, SymbolKind::Test, 5)
}

fn uses(name: &str, line: u32, times: u32) -> NewRef {
    NewRef {
        kind: RefKind::Use,
        name: name.to_owned(),
        line,
        uses: times,
        path: None,
    }
}

fn imports(name: &str, line: u32, path: &str) -> NewRef {
    NewRef {
        kind: RefKind::Import,
        name: name.to_owned(),
        line,
        uses: 1,
        path: Some(path.to_owned()),
    }
}

async fn put(state: &State, project: &str, files: Vec<IndexedFile>) {
    state.replace_graph_files(project, &files).await.unwrap();
}

/// Rows of one of the graph's tables; the name is one of the tests' own literals.
async fn rows(state: &State, table: &'static str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

fn paths<T>(rows: &[T], path: impl Fn(&T) -> &str) -> Vec<&str> {
    rows.iter().map(path).collect()
}

#[tokio::test]
async fn where_puts_code_before_tests_and_keeps_to_the_folder_and_parent() {
    let state = state("graph-where").await;
    let mut nested = defines("Limits");
    nested.parent = Some("config".into());
    put(
        &state,
        P,
        vec![
            file("core/tests/limits.rs", vec![defines("Limits")], vec![]),
            file("protocol/src/limits.rs", vec![nested], vec![]),
            file("core/src/sections.rs", vec![defines("Limits")], vec![]),
            file("corex/src/other.rs", vec![defines("Limits")], vec![]),
        ],
    )
    .await;
    put(
        &state,
        Q,
        vec![file("core/src/q.rs", vec![defines("Limits")], vec![])],
    )
    .await;

    let found = state.graph_where(P, "Limits", None, None).await.unwrap();
    assert_eq!(
        paths(&found, |d| &d.path),
        [
            "core/src/sections.rs",
            "corex/src/other.rs",
            "protocol/src/limits.rs",
            "core/tests/limits.rs"
        ]
    );
    assert_eq!((found[0].line, found[0].end_line), (1, 3));
    assert_eq!(found[0].module, "core::src::sections");
    // A folder is a folder: `core` does not take in `corex`.
    for folder in ["core", "core/"] {
        let found = state
            .graph_where(P, "Limits", None, Some(folder))
            .await
            .unwrap();
        assert_eq!(
            paths(&found, |d| &d.path),
            ["core/src/sections.rs", "core/tests/limits.rs"]
        );
    }
    let found = state
        .graph_where(P, "Limits", Some("config"), None)
        .await
        .unwrap();
    assert_eq!(paths(&found, |d| &d.path), ["protocol/src/limits.rs"]);
    assert!(
        state
            .graph_where(P, "Limitz", None, None)
            .await
            .unwrap()
            .is_empty()
    );

    let many = (0..15)
        .map(|n| file(&format!("m/{n:02}.rs"), vec![defines("new")], vec![]))
        .collect();
    put(&state, P, many).await;
    let found = state.graph_where(P, "new", None, None).await.unwrap();
    assert_eq!(found.len() as i64, WHERE_LIMIT);
}

#[tokio::test]
async fn uses_gives_one_row_a_file_most_uses_first() {
    let state = state("graph-uses").await;
    put(
        &state,
        P,
        vec![
            file("a.rs", vec![defines("Limits")], vec![uses("Limits", 9, 2)]),
            file(
                "b/mod.rs",
                vec![],
                vec![imports("Limits", 1, "crate::a"), uses("Limits", 12, 6)],
            ),
            file(
                "b/only_import.rs",
                vec![],
                vec![imports("Limits", 3, "crate::a")],
            ),
            file("c.rs", vec![], vec![uses("Other", 1, 50)]),
        ],
    )
    .await;
    put(
        &state,
        Q,
        vec![file("q.rs", vec![], vec![uses("Limits", 1, 99)])],
    )
    .await;

    let found = state.graph_uses(P, "Limits", None).await.unwrap();
    let got: Vec<_> = found
        .iter()
        .map(|u| (u.path.as_str(), u.line, u.uses))
        .collect();
    assert_eq!(
        got,
        [
            ("b/mod.rs", 12, 6),
            ("a.rs", 9, 2),
            ("b/only_import.rs", 3, 1)
        ]
    );
    let found = state.graph_uses(P, "Limits", Some("b")).await.unwrap();
    assert_eq!(paths(&found, |u| &u.path), ["b/mod.rs", "b/only_import.rs"]);

    let ids: Vec<i64> = found.iter().map(|u| u.file_id).collect();
    let imported = state.graph_imports(P, &ids).await.unwrap();
    assert_eq!(imported.len(), 2);
    assert!(
        imported
            .iter()
            .all(|i| i.path.as_deref() == Some("crate::a"))
    );
    // Another project's files are not read through their ids.
    assert!(state.graph_imports(Q, &ids).await.unwrap().is_empty());
    assert!(state.graph_imports(P, &[0; 17]).await.is_err());

    let many = (0..20)
        .map(|n| file(&format!("m/{n:02}.rs"), vec![], vec![uses("Limits", 1, 1)]))
        .collect();
    put(&state, P, many).await;
    let found = state.graph_uses(P, "Limits", None).await.unwrap();
    assert_eq!(found.len() as i64, USES_LIMIT);
    assert_eq!(found[0].path, "b/mod.rs");
}

/// `core/src/sections.rs` defines `Limits`, `ThreadConfig` and `new`; `new` is defined in
/// four files, so it says nothing.
async fn sections(name: &str) -> State {
    let state = state(name).await;
    let mut target = file(
        "core/src/sections.rs",
        vec![defines("Limits"), defines("ThreadConfig"), defines("new")],
        vec![uses("Limits", 30, 3)],
    );
    target.symbols.push(test_fn("limits_default"));
    put(
        &state,
        P,
        vec![
            target,
            file(
                "core/src/config/mod.rs",
                vec![],
                vec![
                    uses("Limits", 1, 1),
                    uses("ThreadConfig", 2, 1),
                    uses("new", 3, 9),
                ],
            ),
            file("core/src/live.rs", vec![], vec![uses("ThreadConfig", 4, 1)]),
            file(
                "core/src/fresh.rs",
                vec![defines("new")],
                vec![uses("new", 1, 1)],
            ),
            file("core/src/b.rs", vec![defines("new")], vec![]),
            file("core/src/c.rs", vec![defines("new")], vec![]),
            file(
                "core/tests/config.rs",
                vec![test_fn("one"), test_fn("two")],
                vec![uses("Limits", 1, 1)],
            ),
            file(
                "core/src/helper.rs",
                vec![test_fn("inline")],
                vec![
                    uses("Limits", 1, 1),
                    imports("ThreadConfig", 1, "crate::sections"),
                ],
            ),
        ],
    )
    .await;
    put(
        &state,
        Q,
        vec![file(
            "core/src/elsewhere.rs",
            vec![],
            vec![uses("Limits", 1, 1)],
        )],
    )
    .await;
    state
}

#[tokio::test]
async fn touches_leaves_out_common_names_the_file_itself_and_other_projects() {
    let state = sections("graph-touches").await;
    let found = state
        .graph_touches(P, "core/src/sections.rs", None)
        .await
        .unwrap();
    let got: Vec<_> = found
        .iter()
        .map(|d| (d.path.as_str(), d.is_test, d.names.join(",")))
        .collect();
    assert_eq!(
        got,
        [
            (
                "core/src/config/mod.rs",
                false,
                "Limits,ThreadConfig".into()
            ),
            ("core/src/helper.rs", false, "Limits,ThreadConfig".into()),
            ("core/src/live.rs", false, "ThreadConfig".into()),
            ("core/tests/config.rs", true, "Limits".into()),
        ]
    );
    let found = state
        .graph_touches(P, "core/src/sections.rs", Some("core/tests"))
        .await
        .unwrap();
    assert_eq!(paths(&found, |d| &d.path), ["core/tests/config.rs"]);
    assert!(
        state
            .graph_touches(P, "missing.rs", None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        state
            .graph_touches(Q, "core/src/sections.rs", None)
            .await
            .unwrap()
            .is_empty()
    );

    let many = (0..45)
        .map(|n| file(&format!("m/{n:02}.rs"), vec![], vec![uses("Limits", 1, 1)]))
        .collect();
    put(&state, P, many).await;
    let found = state
        .graph_touches(P, "core/src/sections.rs", None)
        .await
        .unwrap();
    assert_eq!(found.len() as i64, TOUCHES_LIMIT);
}

#[tokio::test]
async fn tests_of_a_file_put_its_own_first() {
    let state = sections("graph-tests").await;
    let found = state
        .graph_tests_of_file(P, "core/src/sections.rs", None)
        .await
        .unwrap();
    let got: Vec<_> = found
        .iter()
        .map(|t| (t.path.as_str(), t.own, t.tests, t.names.len()))
        .collect();
    assert_eq!(
        got,
        [
            ("core/src/sections.rs", true, 1, 0),
            ("core/src/helper.rs", false, 1, 2),
            ("core/tests/config.rs", false, 2, 1),
        ]
    );

    let many = (0..8)
        .map(|n| file(&format!("tests/{n}.rs"), vec![], vec![uses("Limits", 1, 1)]))
        .collect();
    put(&state, P, many).await;
    let found = state
        .graph_tests_of_file(P, "core/src/sections.rs", None)
        .await
        .unwrap();
    assert_eq!(found.len() as i64, TESTS_LIMIT);
    assert_eq!(found[0].path, "core/src/sections.rs");
}

#[tokio::test]
async fn tests_of_a_symbol_put_the_defining_file_first() {
    let state = sections("graph-tests-symbol").await;
    let found = state
        .graph_tests_of_symbol(P, "Limits", None)
        .await
        .unwrap();
    let got: Vec<_> = found
        .iter()
        .map(|t| (t.path.as_str(), t.own, t.tests))
        .collect();
    assert_eq!(
        got,
        [
            ("core/src/sections.rs", true, 1),
            ("core/src/helper.rs", false, 1),
            ("core/tests/config.rs", false, 2),
        ]
    );
    assert_eq!(found[1].names, ["Limits"]);
    let found = state
        .graph_tests_of_symbol(P, "Limits", Some("core/tests"))
        .await
        .unwrap();
    assert_eq!(paths(&found, |t| &t.path), ["core/tests/config.rs"]);
}

#[tokio::test]
async fn changed_with_needs_two_shared_turns() {
    let state = state("graph-changed").await;
    state.create_thread("t", "test").await.unwrap();
    for turn in ["t1", "t2", "t3"] {
        sqlx::query(
            "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at,
                finished_at) VALUES (?, 't', 1, 'interrupted', 0, 0)",
        )
        .bind(turn)
        .execute(&state.pool)
        .await
        .unwrap();
    }
    let changes = [
        ("t1", P, "a.rs"),
        ("t1", P, "b.rs"),
        ("t1", P, "c.rs"),
        ("t2", P, "a.rs"),
        ("t2", P, "b.rs"),
        ("t3", P, "c.rs"),
        ("t3", Q, "a.rs"),
        ("t3", Q, "c.rs"),
    ];
    for (n, (turn, project, path)) in changes.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO changes (change_id, thread_id, turn_id, project, path, kind,
                size_after, created_at) VALUES (?, 't', ?, ?, ?, 'created', 1, 0)",
        )
        .bind(n.to_string())
        .bind(turn)
        .bind(project)
        .bind(path)
        .execute(&state.pool)
        .await
        .unwrap();
    }
    let found = state.graph_changed_with(P, "a.rs").await.unwrap();
    assert_eq!(
        found,
        [ChangedWith {
            path: "b.rs".into(),
            times: 2
        }]
    );
    assert!(
        state
            .graph_changed_with(P, "c.rs")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn replacing_a_file_rewrites_its_rows_only() {
    let state = state("graph-replace").await;
    put(
        &state,
        P,
        vec![
            file("a.rs", vec![defines("Old")], vec![uses("Gone", 1, 1)]),
            file("b.rs", vec![defines("Kept")], vec![uses("Old", 2, 1)]),
        ],
    )
    .await;
    let stamps = state.graph_stamps(P).await.unwrap();
    // A repeated ref is added to the first, not refused.
    let mut again = file(
        "a.rs",
        vec![defines("New")],
        vec![uses("Seen", 4, 1), uses("Seen", 2, 3)],
    );
    again.hash = "f".repeat(64);
    put(&state, P, vec![again]).await;

    assert!(
        state
            .graph_where(P, "Old", None, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(state.graph_uses(P, "Gone", None).await.unwrap().is_empty());
    assert_eq!(
        state
            .graph_where(P, "Kept", None, None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(state.graph_uses(P, "Old", None).await.unwrap().len(), 1);
    let seen = state.graph_uses(P, "Seen", None).await.unwrap();
    assert_eq!((seen[0].line, seen[0].uses), (2, 4));
    let after = state.graph_stamps(P).await.unwrap();
    assert_eq!(after["a.rs"].hash, "f".repeat(64));
    assert_eq!(after["b.rs"], stamps["b.rs"]);
    assert_eq!(
        state.graph_counts(P).await.unwrap(),
        GraphCounts {
            files: 2,
            symbols: 2,
            skipped: 0
        }
    );

    let new = Stamp {
        size: 77,
        modified_ms: 5,
    };
    state
        .restamp_graph_files(P, &[("a.rs".into(), new)])
        .await
        .unwrap();
    let a = state.graph_file(P, "a.rs").await.unwrap().unwrap();
    assert_eq!((a.stamp, a.hash.as_str()), (new, "f".repeat(64).as_str()));
    assert_eq!(
        state.graph_where(P, "New", None, None).await.unwrap().len(),
        1
    );
    assert!(state.graph_file(Q, "a.rs").await.unwrap().is_none());
}

#[tokio::test]
async fn removing_a_file_or_a_project_takes_its_rows() {
    let state = state("graph-remove").await;
    let mut skipped = file("big.rs", vec![], vec![]);
    skipped.skipped = Some(Skipped::TooLarge);
    put(
        &state,
        P,
        vec![
            file("a.rs", vec![defines("A")], vec![uses("B", 1, 1)]),
            file("b.rs", vec![defines("B")], vec![uses("A", 1, 1)]),
            skipped,
        ],
    )
    .await;
    put(
        &state,
        Q,
        vec![file("a.rs", vec![defines("A")], vec![uses("B", 1, 1)])],
    )
    .await;
    let counts = state.graph_counts(P).await.unwrap();
    assert_eq!((counts.files, counts.symbols, counts.skipped), (3, 2, 1));
    assert_eq!(
        state
            .graph_file(P, "big.rs")
            .await
            .unwrap()
            .unwrap()
            .skipped,
        Some(Skipped::TooLarge)
    );

    let removed = state
        .remove_graph_files(P, &["a.rs".into(), "missing.rs".into()])
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(
        (
            rows(&state, "graph_symbols").await,
            rows(&state, "graph_refs").await
        ),
        (2, 2)
    );
    assert!(state.graph_where(Q, "A", None, None).await.unwrap().len() == 1);

    state.remove_graph(P).await.unwrap();
    assert_eq!(state.graph_counts(P).await.unwrap(), GraphCounts::default());
    assert_eq!(state.graph_counts(Q).await.unwrap().files, 1);

    // Removing the saved project takes its graph with it.
    state.remove_project("q").await.unwrap();
    for table in ["graph_files", "graph_symbols", "graph_refs"] {
        assert_eq!(rows(&state, table).await, 0, "{table}");
    }
}

#[tokio::test]
async fn bad_rows_are_refused_and_the_batch_with_them() {
    let state = state("graph-refused").await;
    let long = "x".repeat(129);
    let mut nested = defines("ok");
    nested.end_line = 0;
    let bad = [
        file("/etc/passwd", vec![], vec![]),
        file("", vec![], vec![]),
        file("a.rs", vec![defines(&long)], vec![]),
        file("a.rs", vec![], vec![uses(&long, 1, 1)]),
        file("a.rs", vec![nested], vec![]),
        file("a.rs", vec![], vec![uses("x", 0, 1)]),
        IndexedFile {
            hash: "short".into(),
            ..file("a.rs", vec![], vec![])
        },
    ];
    for bad in bad {
        let batch = [file("good.rs", vec![defines("Good")], vec![]), bad.clone()];
        assert!(
            state.replace_graph_files(P, &batch).await.is_err(),
            "{bad:?}"
        );
    }
    // Only a saved project's folder has a graph.
    let unsaved = [file("good.rs", vec![], vec![])];
    assert!(state.replace_graph_files("/work", &unsaved).await.is_err());
    assert_eq!(rows(&state, "graph_files").await, 0);
    let name = "x".repeat(128);
    put(&state, P, vec![file("a.rs", vec![defines(&name)], vec![])]).await;
    assert_eq!(
        state.graph_where(P, &name, None, None).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn names_and_folders_match_literally() {
    let state = state("graph-literal").await;
    let hostile = "x'); DROP TABLE graph_files; --";
    put(
        &state,
        P,
        vec![
            file(
                "dxr/a.rs",
                vec![defines("axb"), defines("abc")],
                vec![uses("abc", 1, 1)],
            ),
            file(
                "d_r/a.rs",
                vec![defines("a_b"), defines(hostile)],
                vec![uses("a%", 1, 1)],
            ),
            file("d%r/a.rs", vec![defines("a%")], vec![]),
        ],
    )
    .await;
    let where_ = |name: &'static str, folder: Option<&'static str>| {
        let state = &state;
        async move {
            let found = state.graph_where(P, name, None, folder).await.unwrap();
            found.into_iter().map(|d| d.path).collect::<Vec<_>>()
        }
    };
    assert_eq!(where_("a_b", None).await, ["d_r/a.rs"]);
    assert_eq!(where_("a%", None).await, ["d%r/a.rs"]);
    assert_eq!(where_(hostile, None).await, ["d_r/a.rs"]);
    assert!(where_("a", None).await.is_empty());
    assert!(where_("axb", Some("d_r")).await.is_empty());
    assert!(where_("abc", Some("d%r")).await.is_empty());
    assert_eq!(where_("a%", Some("d%r")).await, ["d%r/a.rs"]);
    let found = state.graph_uses(P, "a%", None).await.unwrap();
    assert_eq!(paths(&found, |u| &u.path), ["d_r/a.rs"]);
    assert!(state.graph_uses(P, "a_c", None).await.unwrap().is_empty());
    assert_eq!(rows(&state, "graph_files").await, 3);
}

#[tokio::test]
async fn a_miss_finds_dependencies_and_near_names() {
    let state = state("graph-miss").await;
    let mut manifest = file("Cargo.toml", vec![], vec![]);
    manifest.language = Language::Manifest;
    manifest.refs.push(NewRef {
        kind: RefKind::Dependency,
        name: "serde".into(),
        line: 7,
        uses: 1,
        path: Some("1.0".into()),
    });
    put(
        &state,
        P,
        vec![
            manifest,
            file(
                "a.rs",
                vec![
                    defines("Limits"),
                    defines("limits"),
                    defines("Lime"),
                    defines("Other"),
                ],
                vec![],
            ),
        ],
    )
    .await;
    let found = state.graph_dependencies(P, "serde").await.unwrap();
    assert_eq!(
        found,
        [Dependency {
            path: "Cargo.toml".into(),
            line: 7,
            version: Some("1.0".into())
        }]
    );
    let near = state.graph_nearest_names(P, "Limitz").await.unwrap();
    assert_eq!(near, ["Limits", "limits", "Lime"]);
    assert!(state.graph_nearest_names(P, "").await.unwrap().is_empty());
    assert!(
        state
            .graph_nearest_names(Q, "Limitz")
            .await
            .unwrap()
            .is_empty()
    );
}
