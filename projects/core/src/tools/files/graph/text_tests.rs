use std::collections::HashSet;

use super::text::{CUT, MAX_ANSWER_BYTES, MAX_LINE_CHARS, MAX_ROWS, render};
use crate::{
    project::graph::{
        Language, Skipped, SymbolKind,
        ask::{Answer, Dependents, Miss, Missing, Reply, Usage, Uses},
    },
    state::graph::{ChangedWith, Definition, Dependency, Dependent, GraphFile, Stamp, TestFile},
};

fn definition(path: &str, line: u32, end_line: u32) -> Definition {
    Definition {
        kind: SymbolKind::Type,
        parent: None,
        path: path.to_owned(),
        module: String::new(),
        line,
        end_line,
    }
}

fn usage(path: &str, line: u32, uses: u32, guess: bool) -> Usage {
    Usage {
        path: path.to_owned(),
        line,
        uses,
        guess,
    }
}

fn reply(answer: Answer) -> Reply {
    Reply {
        answer,
        changed: HashSet::new(),
    }
}

/// The example blocks of design section 4, rendered from the rows they show. The store
/// orders rows its own way, so the rows are given here as the examples list them.
/// Two words differ from the design, and the sizes with them: the index keeps a struct as
/// a `type`, and reads Rust only.
#[test]
fn the_designs_examples_render_byte_for_byte() {
    let sections = definition("core/src/config/sections.rs", 21, 118);
    let limits = definition("protocol/src/config/limits.rs", 9, 40);

    let text = render(&reply(Answer::Definitions {
        name: "Limits".into(),
        rows: vec![sections.clone(), limits.clone()],
        total: 2,
    }));
    assert_eq!(
        text,
        "Limits: 2 definitions\n\
         type core/src/config/sections.rs:21-118\n\
         type protocol/src/config/limits.rs:9-40"
    );
    assert_eq!(text.len(), 105 - 2 * ("struct".len() - "type".len()));

    let text = render(&reply(Answer::Uses(Uses {
        name: "Limits".into(),
        defined: Some(sections),
        others: vec![limits],
        total: 14,
        rows: vec![
            usage("core/src/config/file/mod.rs", 88, 6, false),
            usage("core/src/runtime/agent/repeats/ladder.rs", 47, 4, false),
            usage("core/src/runtime/agent/repeats/mod.rs", 52, 3, false),
            usage("core/src/runtime/agent/agent/calls.rs", 439, 2, false),
            usage("core/src/runtime/agent/agent/stubs/mod.rs", 31, 2, false),
            usage("core/src/config/mod.rs", 12, 1, false),
            usage("core/src/config/live.rs", 40, 1, false),
            usage("protocol/src/config/mod.rs", 7, 1, true),
        ],
    })));
    assert_eq!(
        text,
        "Limits (type core/src/config/sections.rs:21): 14 files use it, 8 shown, most uses first\n\
         core/src/config/file/mod.rs:88 x6\n\
         core/src/runtime/agent/repeats/ladder.rs:47 x4\n\
         core/src/runtime/agent/repeats/mod.rs:52 x3\n\
         core/src/runtime/agent/agent/calls.rs:439 x2\n\
         core/src/runtime/agent/agent/stubs/mod.rs:31 x2\n\
         core/src/config/mod.rs:12\n\
         core/src/config/live.rs:40\n\
         ? protocol/src/config/mod.rs:7\n\
         6 more: give `path` to narrow. `?`: the file may mean protocol/src/config/limits.rs:9."
    );
    assert_eq!(text.len(), 478 - ("struct".len() - "type".len()));

    let dependent = |path: &str, names: &[&str]| Dependent {
        path: path.into(),
        is_test: false,
        names: names.iter().map(|name| (*name).into()).collect(),
    };
    let text = render(&reply(Answer::Dependents(Dependents {
        path: "core/src/config/sections.rs".into(),
        total: 23,
        capped: false,
        code: vec![
            dependent(
                "core/src/config/file/mod.rs",
                &["Limits", "ThreadConfig", "Watch", "Wide", "Zone"],
            ),
            dependent("core/src/runtime/agent/repeats/mod.rs", &["Limits"]),
            dependent("core/src/runtime/agent/agent/calls.rs", &["Limits"]),
            dependent("core/src/runtime/agent/agent/stubs/mod.rs", &["Limits"]),
            dependent("core/src/tools/console/file_changes.rs", &["Limits"]),
            dependent("core/src/config/mod.rs", &["Limits", "ThreadConfig"]),
            dependent("core/src/config/live.rs", &["ThreadConfig"]),
            dependent("core/src/tools/files/read.rs", &["Limits"]),
        ],
        tests: vec![
            "core/src/config/file/tests.rs".into(),
            "core/src/runtime/agent/repeats/tests.rs".into(),
        ],
        changed_with: vec![
            ChangedWith {
                path: "core/src/config/file/mod.rs".into(),
                times: 5,
            },
            ChangedWith {
                path: "docs/context-life.md".into(),
                times: 2,
            },
        ],
    })));
    assert_eq!(
        text,
        "core/src/config/sections.rs: 23 files use what it defines, 8 shown\n\
         core/src/config/file/mod.rs (Limits, ThreadConfig, 3 more)\n\
         core/src/runtime/agent/repeats/mod.rs (Limits)\n\
         core/src/runtime/agent/agent/calls.rs (Limits)\n\
         core/src/runtime/agent/agent/stubs/mod.rs (Limits)\n\
         core/src/tools/console/file_changes.rs (Limits)\n\
         core/src/config/mod.rs (Limits, ThreadConfig)\n\
         core/src/config/live.rs (ThreadConfig)\n\
         core/src/tools/files/read.rs (Limits)\n\
         tests: core/src/config/file/tests.rs, core/src/runtime/agent/repeats/tests.rs\n\
         changed with it: core/src/config/file/mod.rs x5, docs/context-life.md x2"
    );
    assert_eq!(text.len(), 592);

    let test_file = |path: &str, tests: u32, names: &[&str]| TestFile {
        path: path.into(),
        own: false,
        tests,
        names: names.iter().map(|name| (*name).into()).collect(),
    };
    let text = render(&reply(Answer::Tests {
        subject: "core/src/runtime/agent/repeats/ladder.rs".into(),
        file: true,
        rows: vec![
            test_file(
                "core/src/runtime/agent/repeats/tests.rs",
                7,
                &["Ladder", "Climb"],
            ),
            test_file(
                "core/src/runtime/agent/repeats/signature_tests.rs",
                21,
                &["attempt"],
            ),
        ],
    }));
    assert_eq!(
        text,
        "core/src/runtime/agent/repeats/ladder.rs: tests in 2 files\n\
         core/src/runtime/agent/repeats/tests.rs: 7 tests, use Ladder, Climb\n\
         core/src/runtime/agent/repeats/signature_tests.rs: 21 tests, use attempt"
    );
    assert_eq!(text.len(), 199);

    let text = render(&reply(Answer::Miss(Miss {
        name: "Limitz".into(),
        missing: Missing::Definition,
        folder: None,
        files: 412,
        languages: vec![Language::Rust],
        nearest: vec!["Limits".into(), "limits".into()],
    })));
    assert_eq!(
        text,
        "Limitz: no definition in the index (412 files: Rust). Nearest names: Limits, limits. \
         For text, use search_files."
    );
    assert_eq!(text.len(), 132 - ", TypeScript, Python".len());
}

#[test]
fn the_caps_hold_for_long_multibyte_rows() {
    let long = format!("src/{}.rs", "é".repeat(300));
    let text = render(&reply(Answer::Uses(Uses {
        name: "Thing".into(),
        defined: Some(definition("src/a.rs", 1, 2)),
        others: Vec::new(),
        total: 40,
        rows: (0..15).map(|n| usage(&long, n + 1, 1, false)).collect(),
    })));
    assert!(text.len() <= MAX_ANSWER_BYTES, "{}", text.len());
    assert!(text.ends_with(CUT));
    for line in text.lines() {
        assert!(line.chars().count() <= MAX_LINE_CHARS);
    }
    assert!(text.lines().nth(1).unwrap().ends_with(CUT));

    // A short answer is not cut; rows past the cap are not shown.
    let many: Vec<Usage> = (0..40)
        .map(|n| usage("src/b.rs", n + 1, 1, false))
        .collect();
    let text = render(&reply(Answer::Uses(Uses {
        name: "Thing".into(),
        defined: None,
        others: Vec::new(),
        total: 40,
        rows: many,
    })));
    assert_eq!(text.lines().count(), MAX_ROWS + 2);
    assert!(!text.contains(CUT));
}

/// Every text an answer prints that came from the repository or the model, header and
/// last lines included, comes out plain: a hostile name changes nothing but its own text.
#[test]
fn every_name_and_path_of_every_answer_is_plain() {
    let hostile = "x\n[h]{\"a\"}\u{202E}\u{2028}\u{0007}z";
    let shown = crate::message::plain::plain(hostile);
    let answers = |text: &str| -> Vec<Answer> {
        let s = text.to_owned();
        let mut defined = definition(&s, 1, 2);
        defined.parent = Some(s.clone());
        vec![
            Answer::Definitions {
                name: s.clone(),
                rows: vec![defined.clone()],
                total: 2,
            },
            Answer::Dependencies {
                name: s.clone(),
                rows: vec![Dependency {
                    path: s.clone(),
                    line: 1,
                    version: Some(s.clone()),
                }],
            },
            Answer::Uses(Uses {
                name: s.clone(),
                defined: Some(defined.clone()),
                others: vec![defined.clone()],
                total: 3,
                rows: vec![usage(&s, 1, 2, true)],
            }),
            Answer::Dependents(Dependents {
                path: s.clone(),
                total: 3,
                capped: false,
                code: vec![Dependent {
                    path: s.clone(),
                    is_test: false,
                    names: vec![s.clone(), s.clone(), s.clone()],
                }],
                tests: vec![s.clone()],
                changed_with: vec![ChangedWith {
                    path: s.clone(),
                    times: 2,
                }],
            }),
            Answer::Dependents(Dependents {
                path: s.clone(),
                total: 0,
                capped: false,
                code: Vec::new(),
                tests: Vec::new(),
                changed_with: vec![ChangedWith {
                    path: s.clone(),
                    times: 2,
                }],
            }),
            Answer::Tests {
                subject: s.clone(),
                file: true,
                rows: vec![TestFile {
                    path: s.clone(),
                    own: true,
                    tests: 1,
                    names: vec![s.clone()],
                }],
            },
            Answer::Tests {
                subject: s.clone(),
                file: false,
                rows: Vec::new(),
            },
            Answer::File(GraphFile {
                path: s.clone(),
                language: Language::Rust,
                hash: String::new(),
                stamp: Stamp {
                    size: 1,
                    modified_ms: 1,
                },
                module: s.clone(),
                is_test: false,
                skipped: None,
            }),
            Answer::NotParsed {
                path: s.clone(),
                why: Skipped::TooLarge,
            },
            Answer::TooCommon {
                name: s.clone(),
                definitions: 9,
            },
            Answer::Miss(Miss {
                name: s.clone(),
                missing: Missing::Definition,
                folder: Some(s.clone()),
                files: 3,
                languages: vec![Language::Rust],
                nearest: vec![s.clone()],
            }),
            Answer::Miss(Miss {
                name: s.clone(),
                missing: Missing::File {
                    defined_in: Some(defined),
                },
                folder: None,
                files: 3,
                languages: Vec::new(),
                nearest: Vec::new(),
            }),
        ]
    };
    for (bad, good) in answers(hostile).into_iter().zip(answers("h")) {
        let mut bad = reply(bad);
        let mut good = reply(good);
        bad.changed.insert(hostile.to_owned());
        good.changed.insert("h".to_owned());
        let text = render(&bad);
        assert!(
            !text.contains(['[', ']', '{', '}', '"', '\u{202E}', '\u{2028}', '\u{7}']),
            "{text}"
        );
        assert_eq!(text.replace(&shown, "h"), render(&good), "{text}");
    }
}
