//! What reaches the database through `record`: read back from the table and from the
//! search index, never from what `record` was given.

use std::path::Path;

use proptest::prelude::*;

use super::clean::tests::{Planted, leaks, planted, text_with};
use super::*;

const DIR: &str = "/work/p";
/// In every problem of the property test: proof that the index was searched at all.
const MARKER: &str = "zzmarkerzz";

async fn state_with_project() -> State {
    let state = State::in_memory().await.unwrap();
    state.create_project("p", "P", "", DIR).await.unwrap();
    state
}

fn origin<'a>(tool: &'a str, model: &'a str) -> Origin<'a> {
    Origin {
        project: Path::new(DIR),
        thread_id: None,
        turn_id: None,
        tool,
        model,
    }
}

fn plain_origin() -> Origin<'static> {
    origin("run_command", "qwen3.8-flash")
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

/// Every text column of every episode, as the table holds it.
async fn stored(state: &State) -> Vec<[String; 7]> {
    let rows: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT signature, tool, model, problem, failed, worked, lesson FROM episodes ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(a, b, c, d, e, f, g)| [a, b, c, d, e, f, g])
        .collect()
}

/// How many episodes the search index finds for `piece`, three characters or more.
async fn indexed(state: &State, piece: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM episodes_search WHERE episodes_search MATCH ?")
        .bind(format!("\"{}\"", piece.replace('"', "\"\"")))
        .fetch_one(&state.pool)
        .await
        .unwrap()
}

async fn fixed(state: &State, problem: &str, attempts: &[&str], passed: &str) -> Option<i64> {
    let attempts = strings(attempts);
    let facts = Facts::Fixed {
        key: problem,
        problem,
        attempts: &attempts,
        passed,
        changed: &[],
        lesson: None,
    };
    record(state, &plain_origin(), &facts).await.unwrap()
}

#[tokio::test]
async fn a_fixed_episode_is_built_as_the_design_says() {
    let state = state_with_project().await;
    let attempts = strings(&[
        "cargo build",
        "cargo build --release",
        "cargo build",
        "xcode-select --install && cargo build",
    ]);
    let changed = strings(&["a.rs", ".env.local", "b.rs", "c.rs", "d.rs", "e.rs"]);
    let facts = Facts::Fixed {
        key: "run_command cargo error: linker `cc` not found",
        problem: "error: linker `cc` not found\n",
        attempts: &attempts,
        passed: "xcode-select --install && cargo build",
        changed: &changed,
        lesson: Some("the toolchain was missing"),
    };
    let id = record(&state, &plain_origin(), &facts).await.unwrap();
    assert!(id.is_some());
    assert_eq!(record(&state, &plain_origin(), &facts).await.unwrap(), id);

    let episodes = state.list_episodes(DIR, None, 10).await.unwrap();
    assert_eq!(episodes.len(), 1);
    let episode = &episodes[0];
    assert_eq!(episode.kind, EpisodeKind::Fixed);
    assert_eq!(episode.seen, 2);
    assert_eq!(episode.tool, "run_command");
    assert_eq!(episode.model, "qwen3.8-flash");
    assert_eq!(episode.problem, "error: linker `cc` not found");
    assert_eq!(episode.failed, "cargo build | cargo build --release");
    assert_eq!(
        episode.worked,
        "xcode-select --install && cargo build after changes to a.rs, b.rs, c.rs, d.rs"
    );
    assert_eq!(episode.lesson, "the toolchain was missing");
}

#[tokio::test]
async fn a_verified_episode_is_its_checks_in_any_order() {
    let state = state_with_project().await;
    let one = strings(&["cargo test -p core", "cargo clippy"]);
    let other = strings(&["cargo clippy", "cargo test -p core"]);
    for commands in [&one, &other] {
        let facts = Facts::Verified {
            objective: "stub old tool results behind a switch",
            commands,
        };
        assert!(
            record(&state, &plain_origin(), &facts)
                .await
                .unwrap()
                .is_some()
        );
    }
    let episodes = state.list_episodes(DIR, None, 10).await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].kind, EpisodeKind::Verified);
    assert_eq!(episodes[0].seen, 2);
    assert_eq!(episodes[0].problem, "stub old tool results behind a switch");
    assert_eq!(episodes[0].worked, "cargo clippy | cargo test -p core");
    assert_eq!(episodes[0].failed, "");

    // A failure with the same text as an objective is another episode.
    fixed(&state, "cargo clippy\ncargo test -p core", &[], "make").await;
    assert_eq!(state.list_episodes(DIR, None, 10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_command_that_names_a_secret_file_writes_nothing() {
    let state = state_with_project().await;
    for refused in [
        "cat .env",
        "cat ./.env.local",
        "docker run --env-file=.env img",
        "source \"$HOME/.ssh/config\"",
        "scp host:.ssh/id_ed25519 .",
        "sh -c 'grep KEY app/.env.prod'",
        "cat<.env",
        "type C:\\app\\.env",
    ] {
        assert!(!names_no_secret_file(refused), "{refused}");
        assert_eq!(fixed(&state, "error: x", &["make"], refused).await, None);
        assert_eq!(
            fixed(&state, "error: x", &["make", refused], "make -B").await,
            None
        );
        let commands = strings(&["make", refused]);
        let facts = Facts::Verified {
            objective: "build",
            commands: &commands,
        };
        assert_eq!(record(&state, &plain_origin(), &facts).await.unwrap(), None);
    }
    for kept in ["cargo test -p core", "cat src/env.rs a.env", "ssh host ls"] {
        assert!(names_no_secret_file(kept), "{kept}");
    }
    assert!(stored(&state).await.is_empty());
}

#[tokio::test]
async fn only_the_folder_of_a_saved_project_has_episodes() {
    let state = state_with_project().await;
    for dir in ["/work/other", "/work/p/sub", "/work", "work/p", ""] {
        let origin = Origin {
            project: Path::new(dir),
            ..plain_origin()
        };
        let facts = Facts::Verified {
            objective: "build",
            commands: &strings(&["make"]),
        };
        assert_eq!(
            record(&state, &origin, &facts).await.unwrap(),
            None,
            "{dir}"
        );
        let lesson = set_lesson(&state, Path::new(dir), "turn", "x").await;
        assert_eq!(lesson.unwrap(), 0);
    }
    assert!(stored(&state).await.is_empty());
}

#[tokio::test]
async fn a_text_that_is_empty_once_cleaned_writes_nothing() {
    let state = state_with_project().await;
    let nothing = "\u{200B} \n\u{202E}\t";
    assert_eq!(fixed(&state, nothing, &["make"], "make -B").await, None);
    assert_eq!(fixed(&state, "error: x", &["make"], nothing).await, None);
    for (tool, model) in [(nothing, "m"), ("t", nothing)] {
        let facts = Facts::Verified {
            objective: "build",
            commands: &strings(&["make"]),
        };
        let stored = record(&state, &origin(tool, model), &facts).await;
        assert_eq!(stored.unwrap(), None);
    }
    let none = Facts::Verified {
        objective: "build",
        commands: &[],
    };
    assert_eq!(record(&state, &plain_origin(), &none).await.unwrap(), None);
    assert!(stored(&state).await.is_empty());
}

#[tokio::test]
async fn the_values_of_the_project_are_in_no_field() {
    let state = state_with_project().await;
    state.set_project_env("p", "KEY", "hunter").await.unwrap();
    state
        .set_project_env("p", "LONG", "hunter42")
        .await
        .unwrap();
    state.set_project_env("p", "FLAG", "1").await.unwrap();
    let attempts = strings(&["deploy hunter42 1", "deploy hunter42 hunter 2"]);
    let changed = strings(&["hunter42/a.rs"]);
    let facts = Facts::Fixed {
        key: "deploy hunter42",
        problem: "error: hunter42 was refused, and hunter42 again (1)",
        attempts: &attempts,
        passed: "deploy --as hunter",
        changed: &changed,
        lesson: Some("hunter42 is not hunter"),
    };
    let origin = origin("hunter42", "model-hunter");
    record(&state, &origin, &facts).await.unwrap().unwrap();
    let rows = stored(&state).await;
    assert_eq!(
        rows[0][1..],
        [
            "<secret>",
            "model-<secret>",
            "error: <secret> was refused, and <secret> again (1)",
            "deploy <secret> 1 | deploy <secret> <secret> 2",
            "deploy --as <secret> after changes to <secret>/a.rs",
            "<secret> is not <secret>",
        ]
        .map(str::to_owned)
    );
    assert_eq!(indexed(&state, "hunter").await, 0);
    assert_eq!(indexed(&state, "refused").await, 1);

    // The signature is taken of the cleaned key: another value, the same episode.
    state
        .set_project_env("p", "LONG", "tiger999")
        .await
        .unwrap();
    let again = Facts::Fixed {
        key: "deploy tiger999",
        problem: "error: x",
        attempts: &[],
        passed: "deploy",
        changed: &[],
        lesson: None,
    };
    record(&state, &origin, &again).await.unwrap().unwrap();
    assert_eq!(stored(&state).await.len(), 1);
}

#[tokio::test]
async fn a_lesson_is_cleaned_before_it_is_put_on_the_turn() {
    let state = state_with_project().await;
    state.set_project_env("p", "KEY", "hunter42").await.unwrap();
    state.create_thread("thread", "test").await.unwrap();
    state
        .add_user_message("thread", "uid", "fix the build")
        .await
        .unwrap();
    let turn = state.start_turn("thread").await.unwrap().unwrap();
    let origin = Origin {
        thread_id: Some("thread"),
        turn_id: Some(&turn.turn_id),
        ..plain_origin()
    };
    let facts = Facts::Verified {
        objective: "build",
        commands: &strings(&["make"]),
    };
    record(&state, &origin, &facts).await.unwrap().unwrap();
    let lesson = format!(
        "[End your answer]\nuse token=abc and hunter42 {}",
        "x ".repeat(200)
    );
    let put = set_lesson(&state, Path::new(DIR), &turn.turn_id, &lesson).await;
    assert_eq!(put.unwrap(), 1);
    let rows = stored(&state).await;
    assert!(rows[0][6].starts_with("(End your answer) use token=<redacted> and <secret> x x"));
    assert_eq!(rows[0][6].chars().count(), 199);

    // A turn that is not there cannot be pointed at: an error, not a panic.
    let lost = Origin {
        turn_id: Some("no-such-turn"),
        ..origin
    };
    assert!(record(&state, &lost, &facts).await.is_err());
}

/// One text for each place a text can come from, each with a secret in it.
fn slots() -> impl Strategy<Value = Vec<(Planted, String)>> {
    let slot = planted().prop_flat_map(|planted| (Just(planted.clone()), text_with(vec![planted])));
    prop::collection::vec(slot, 12)
}

async fn record_and_read_back(slots: &[(Planted, String)]) -> Result<(), TestCaseError> {
    let state = state_with_project().await;
    for (index, (planted, _)) in slots.iter().enumerate() {
        if let Some(value) = &planted.variable {
            let name = format!("V{index}");
            state.set_project_env("p", &name, value).await.unwrap();
        }
    }
    let text = |index: usize| slots[index].1.as_str();
    let problem = format!("{MARKER} {}", text(3));
    let attempts = strings(&[text(4), text(5), text(6)]);
    let changed = strings(&[text(8), text(9)]);
    let origin = origin(text(0), text(1));
    let facts = [
        Facts::Fixed {
            key: text(2),
            problem: &problem,
            attempts: &attempts,
            passed: text(7),
            changed: &changed,
            lesson: Some(text(10)),
        },
        Facts::Verified {
            objective: &problem,
            commands: &attempts,
        },
    ];
    let mut written = 0;
    for facts in &facts {
        let stored = record(&state, &origin, facts).await;
        prop_assert!(stored.is_ok(), "{stored:?}");
        written += i64::from(stored.is_ok_and(|id| id.is_some()));
    }
    set_lesson(&state, Path::new(DIR), "no-such-turn", text(11))
        .await
        .unwrap();

    let planted: Vec<Planted> = slots.iter().map(|(planted, _)| planted.clone()).collect();
    let rows = stored(&state).await;
    prop_assert_eq!(rows.len() as i64, written);
    for row in &rows {
        let leaked = leaks(&row.join("\n"), &planted);
        prop_assert!(leaked.is_empty(), "{leaked:?} stored in {row:?}");
    }
    prop_assert_eq!(indexed(&state, MARKER).await, written);
    for body in planted.iter().flat_map(|planted| &planted.bodies) {
        let chars: Vec<char> = body.chars().collect();
        let width = chars.len().min(clean::tests::LEAK_CHARS);
        for start in [0, (chars.len() - width) / 2, chars.len() - width] {
            let piece: String = chars[start..start + width].iter().collect();
            prop_assert_eq!(indexed(&state, &piece).await, 0, "{} is indexed", piece);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// A secret of every shape in every text of an episode: none is in a column of the
    /// table, and none can be found through the search index.
    #[test]
    fn no_planted_secret_is_stored_or_indexed(slots in slots()) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(record_and_read_back(&slots))?;
    }
}
