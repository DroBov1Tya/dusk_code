use std::fs;

use super::*;
use crate::{
    state::graph::{Author, Record},
    testing::temp_dir,
};

async fn saved(name: &str) -> (State, PathBuf, String) {
    let dir = temp_dir(name);
    fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let state = State::in_memory().await.unwrap();
    let project = dir.to_str().unwrap().to_owned();
    state.create_project("p", "p", "", &project).await.unwrap();
    (state, dir, project)
}

const LONG: Duration = Duration::from_secs(30);

#[tokio::test]
async fn only_changed_files_are_read_and_their_facts_go_stale() {
    let (state, dir, project) = saved("graph-refresh").await;
    fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
    fs::write(dir.join("b.bin"), b"\0\n\n\n").unwrap();
    fs::write(dir.join(".env"), "KEY=1\n").unwrap();
    let first = refresh(&state, &project, &dir, &[], LONG).await.unwrap();
    assert_eq!((first.files, first.read, first.pending), (3, 3, 0));
    let a = state.graph_file(&project, "a.txt").await.unwrap().unwrap();
    assert_eq!((a.lines, a.hashed), (2, true));
    let bin = state.graph_file(&project, "b.bin").await.unwrap().unwrap();
    assert_eq!((bin.lines, bin.hashed), (0, true));
    // A file of secrets is listed with its size, never hashed, and takes no facts.
    let env = state.graph_file(&project, ".env").await.unwrap().unwrap();
    assert!(!env.hashed);

    let hash = measure(&dir, "a.txt").unwrap().hash;
    let record = Record {
        summary: Some("Two words.".into()),
        ..Record::default()
    };
    let author = Author {
        thread: None,
        by_user: false,
    };
    state
        .graph_record(&project, "a.txt", &hash, &record, author)
        .await
        .unwrap();

    // Nothing moved: nothing is read again.
    let again = refresh(&state, &project, &dir, &[], LONG).await.unwrap();
    assert_eq!(again.read, 0);

    fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    fs::remove_file(dir.join("b.bin")).unwrap();
    let changed = refresh(&state, &project, &dir, &[], LONG).await.unwrap();
    assert_eq!((changed.read, changed.removed), (1, 1));
    let a = state.graph_file(&project, "a.txt").await.unwrap().unwrap();
    assert_eq!(a.lines, 3);
    assert_eq!(a.summary, "Two words.");
    assert!(a.stale);
    assert!(state.graph_file(&project, "b.bin").await.unwrap().is_none());
}

#[tokio::test]
async fn a_spent_budget_leaves_files_for_the_next_refresh() {
    let (state, dir, project) = saved("graph-budget").await;
    for n in 0..5 {
        fs::write(dir.join(format!("f{n}")), "x\n").unwrap();
    }
    let none = refresh(&state, &project, &dir, &[], Duration::ZERO)
        .await
        .unwrap();
    assert_eq!((none.read, none.pending), (0, 5));
    assert!(state.graph_map(&project, None).await.unwrap().is_empty());
    // A refresh that left work is not a full one: the next read asks for another.
    let within = refresh_within(&state, &dir, &[], LONG, LONG)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(within.read, 5);
    let fresh = refresh_within(&state, &dir, &[], LONG, LONG)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fresh, Refreshed::default());
}

#[tokio::test]
async fn a_folder_that_is_not_a_saved_project_has_no_graph() {
    let state = State::in_memory().await.unwrap();
    let dir = temp_dir("graph-unsaved");
    fs::create_dir_all(&dir).unwrap();
    let refreshed = refresh_within(&state, &dir, &[], LONG, LONG).await.unwrap();
    assert!(refreshed.is_none());
}

#[tokio::test]
async fn in_a_repository_ignored_files_and_skipped_folders_are_left_out() {
    let (state, dir, project) = saved("graph-git").await;
    let init = crate::project::git::run(&dir, &["init", "-q"], LONG).await;
    if !init.is_ok_and(|output| output.success) {
        // No git program here; the walk without git is checked above.
        return;
    }
    fs::write(dir.join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(dir.join("build")).unwrap();
    fs::write(dir.join("build/out"), "x").unwrap();
    fs::create_dir_all(dir.join("work")).unwrap();
    fs::write(dir.join("work/tmp"), "x").unwrap();
    fs::write(dir.join("main.rs"), "fn main() {}\n").unwrap();
    #[cfg(unix)]
    {
        let outside = temp_dir("graph-git-outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), dir.join("link")).unwrap();
    }
    refresh(&state, &project, &dir, &[dir.join("work")], LONG)
        .await
        .unwrap();
    let paths: Vec<String> = state
        .graph_map(&project, None)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    assert_eq!(paths, [".gitignore", "main.rs"]);
}
