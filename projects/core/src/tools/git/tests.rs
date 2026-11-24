//! The boundaries of an agent's branch: where its tools act, what a confined command may
//! write, the conflict flow that never touches the user's folder, and the refusals.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use dusk_protocol::tools::{ApprovalRequest, Decision};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::{follow, work};
use crate::{
    project::git,
    runtime::ThreadEvent,
    state::State,
    testing::{temp_dir, temp_state, tool_context},
    tools::{Approvals, Progress, ToolContext, ToolScope},
};

fn run(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A repository on `main` with one commit, its workspace, and a chat.
async fn repository(name: &str) -> (Arc<State>, ToolScope, PathBuf) {
    let root = temp_dir(name);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(root.join("work")).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    run(&repo, &["config", "user.email", "test@example.invalid"]);
    run(&repo, &["config", "user.name", "test"]);
    run(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
    run(&repo, &["add", "a.txt"]);
    run(&repo, &["commit", "-qm", "first"]);
    let state = Arc::new(temp_state(name).await);
    state.create_thread("chat", "test").await.unwrap();
    let scope = ToolScope::new(&repo, &root.join("work")).unwrap();
    let repo = scope.cwd().to_owned();
    (state, scope, repo)
}

/// A context whose approval requests are answered with `answer` and kept.
fn answering(
    state: &Arc<State>,
    scope: &ToolScope,
    answer: Decision,
) -> (ToolContext, Arc<Mutex<Vec<ApprovalRequest>>>) {
    answering_after(state, scope, answer, || {})
}

/// `answering`, with `meanwhile` done before each answer, as the user may while a
/// request waits.
fn answering_after(
    state: &Arc<State>,
    scope: &ToolScope,
    answer: Decision,
    meanwhile: impl Fn() + Send + 'static,
) -> (ToolContext, Arc<Mutex<Vec<ApprovalRequest>>>) {
    let approvals = Arc::new(Approvals::default());
    let (events, mut heard) = mpsc::unbounded_channel();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (kept, answers) = (Arc::clone(&asked), Arc::clone(&approvals));
    tokio::spawn(async move {
        while let Some(event) = heard.recv().await {
            if let ThreadEvent::Approval(request) = event {
                kept.lock().push(request.clone());
                meanwhile();
                answers.answer(request.id, answer);
            }
        }
    });
    let context = ToolContext {
        progress: Some(Progress {
            call_id: "branch-test".into(),
            events,
        }),
        approvals: Some(approvals),
        state: Some(Arc::clone(state)),
        thread_id: "chat".into(),
        ..tool_context(scope)
    };
    (context, asked)
}

fn commit_in(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    run(dir, &["add", "--all"]);
    run(dir, &["commit", "-qm", "change"]);
}

#[tokio::test]
async fn a_thread_works_in_its_worktree_and_merges_back_with_a_merge_commit() {
    let (state, home, repo) = repository("branch-flow").await;
    let made = work::start(&state, "chat", "chat", &home, Some("Fix it"))
        .await
        .unwrap();
    assert_eq!(made.branch, "agent/fix-it");
    assert_eq!(
        (made.base.as_str(), made.worktree_of.as_str()),
        ("main", repo.to_str().unwrap())
    );
    let worktree = PathBuf::from(&made.dir);
    assert!(
        !worktree.starts_with(&repo),
        "beside the repository, never in it"
    );
    assert!(
        work::start(&state, "chat", "chat", &home, None)
            .await
            .is_err(),
        "one branch at a time"
    );

    // The tools follow the record: there, with the project's lookups kept on the project.
    let home = Arc::new(home);
    let there = follow::scope_for(&state, "chat", &home, &home).await;
    assert_eq!(there.cwd(), worktree);
    assert_eq!(there.project_dir(), repo);
    assert_eq!(there.workspace(), home.workspace());
    let again = follow::scope_for(&state, "chat", &home, &there).await;
    assert!(Arc::ptr_eq(&there, &again), "kept while it fits");
    let repository = git::repository(&worktree).await.unwrap();
    let writable = there.git_writable();
    assert!(writable.contains(&repository.common.join("objects")));
    assert!(writable.contains(&repository.own));
    for closed in ["", "hooks", "config", "refs/heads", "refs"] {
        let path = repository.common.join(closed);
        assert!(!writable.contains(&path), "{closed:?} is writable");
    }

    std::fs::write(worktree.join("b.txt"), "new\n").unwrap();
    let committed = work::commit(&made, "add b").await.unwrap();
    assert!(committed.contains("1 commit ahead"), "{committed}");
    assert!(
        work::status(&made)
            .await
            .unwrap()
            .contains("Nothing uncommitted")
    );

    let (context, asked) = answering(&state, &there, Decision::Once);
    let work::Done::Finished(text) = work::merge(&context, &state, &made).await.unwrap() else {
        panic!("the merge waits");
    };
    assert!(text.contains("Merged agent/fix-it into main"), "{text}");
    // A merge into the user's branch is asked every time: no yes for the session.
    let asked = asked.lock().clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].session.is_none());
    assert!(repo.join("b.txt").is_file());
    assert!(!worktree.exists());
    assert!(!git::names(&repo).await.unwrap().contains(&made.branch));
    let parents = std::process::Command::new("git")
        .args(["rev-list", "--parents", "-n", "1", "HEAD"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let parents = String::from_utf8(parents.stdout).unwrap();
    assert_eq!(parents.split_whitespace().count(), 3, "a merge commit");
    assert_eq!(state.thread_worktree("chat").await.unwrap(), None);
    let back = follow::scope_for(&state, "chat", &home, &there).await;
    assert!(Arc::ptr_eq(&back, &home), "back in its own folder");
}

#[tokio::test]
async fn conflicts_are_resolved_in_the_worktree_never_in_the_users_folder() {
    let (state, home, repo) = repository("branch-conflict").await;
    let made = work::start(&state, "chat", "chat", &home, Some("c"))
        .await
        .unwrap();
    let worktree = PathBuf::from(&made.dir);
    commit_in(&worktree, "a.txt", "one\nmine\n");
    commit_in(&repo, "a.txt", "one\ntheirs\n");
    let (context, asked) = answering(&state, &home, Decision::Once);

    let work::Done::Waits(text) = work::merge(&context, &state, &made).await.unwrap() else {
        panic!("merged over a conflict");
    };
    assert!(text.contains("a.txt"), "{text}");
    assert!(
        asked.lock().is_empty(),
        "nothing asked before the conflict is resolved"
    );
    assert!(
        !git::merging(&repo).await,
        "the user's folder is not mid-merge"
    );
    assert!(git::merging(&worktree).await);
    assert!(
        work::merge(&context, &state, &made).await.is_err(),
        "a merge waits"
    );
    assert!(
        work::commit(&made, "resolve").await.is_err(),
        "conflicts are left"
    );

    // Aborted, the branch is as before; merged again, the conflict is back.
    work::abort(&made).await.unwrap();
    assert!(!git::merging(&worktree).await);
    assert!(work::abort(&made).await.is_err(), "no merge waits");
    let work::Done::Waits(_) = work::merge(&context, &state, &made).await.unwrap() else {
        panic!("merged over a conflict");
    };
    std::fs::write(worktree.join("a.txt"), "one\nboth\n").unwrap();
    work::commit(&made, "resolve").await.unwrap();
    let work::Done::Finished(_) = work::merge(&context, &state, &made).await.unwrap() else {
        panic!("the merge still waits");
    };
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).unwrap(),
        "one\nboth\n"
    );
}

#[tokio::test]
async fn a_merge_or_an_abandon_is_refused_where_it_would_lose_or_mix_work() {
    let (state, home, repo) = repository("branch-refusals").await;
    let made = work::start(&state, "chat", "chat", &home, Some("r"))
        .await
        .unwrap();
    let worktree = PathBuf::from(&made.dir);
    std::fs::write(worktree.join("b.txt"), "b\n").unwrap();
    let (yes, _) = answering(&state, &home, Decision::Once);
    let (no, _) = answering(&state, &home, Decision::Deny);

    let error = work::merge(&yes, &state, &made).await.err().unwrap();
    assert!(
        error.to_string().contains("commit the work first"),
        "{error}"
    );
    work::commit(&made, "b").await.unwrap();

    // The user's tracked changes are never mixed into a merge.
    std::fs::write(repo.join("a.txt"), "edited\n").unwrap();
    let error = work::merge(&yes, &state, &made).await.err().unwrap();
    assert!(error.to_string().contains("uncommitted changes"), "{error}");
    run(&repo, &["checkout", "--", "a.txt"]);

    // The user's folder on another branch is not merged into.
    run(&repo, &["switch", "-q", "-c", "other"]);
    let error = work::merge(&yes, &state, &made).await.err().unwrap();
    assert!(error.to_string().contains("is on other now"), "{error}");
    run(&repo, &["switch", "-q", "main"]);

    // A no leaves everything where it was.
    assert!(work::merge(&no, &state, &made).await.is_err());
    assert!(!repo.join("b.txt").exists());
    assert!(worktree.is_dir());
    assert!(work::abandon(&no, &state, &made).await.is_err());
    assert!(worktree.is_dir());

    // Abandoned with a yes: the uncommitted work is committed, the branch is kept.
    std::fs::write(worktree.join("c.txt"), "c\n").unwrap();
    let work::Done::Finished(text) = work::abandon(&yes, &state, &made).await.unwrap() else {
        panic!("not abandoned");
    };
    assert!(text.contains("kept"), "{text}");
    assert!(!worktree.exists());
    assert!(git::names(&repo).await.unwrap().contains(&made.branch));
    let kept = std::process::Command::new("git")
        .args(["show", "--name-only", "--format=", &made.branch])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(String::from_utf8(kept.stdout).unwrap().contains("c.txt"));
    assert_eq!(state.thread_worktree("chat").await.unwrap(), None);

    // Branches another thread made are out of reach; a folder outside git cannot branch.
    state.create_thread("other", "test").await.unwrap();
    work::start(&state, "other", "other", &home, Some("o"))
        .await
        .unwrap();
    assert!(work::find(&state, "chat", Some("other")).await.is_err());
    assert!(work::find(&state, "chat", None).await.is_err());
    assert!(work::find(&state, "other", None).await.is_ok());
    let plain = temp_dir("branch-plain");
    std::fs::create_dir_all(&plain).unwrap();
    let outside = ToolScope::new(&plain, &plain).unwrap();
    assert!(
        work::start(&state, "chat", "chat", &outside, None)
            .await
            .is_err()
    );
}

/// A confined `git commit` in the worktree works with the git data the scope opens, and
/// fails without it; the shared hooks stay closed either way.
#[tokio::test]
async fn a_confined_commit_writes_only_the_git_data_it_needs() {
    if !crate::platform::sandbox_available() {
        return;
    }
    let (state, home, _) = repository("branch-confined").await;
    let made = work::start(&state, "chat", "chat", &home, Some("s"))
        .await
        .unwrap();
    let home = Arc::new(home);
    let there = follow::scope_for(&state, "chat", &home, &home).await;
    let worktree = there.cwd().to_owned();
    std::fs::write(worktree.join("b.txt"), "b\n").unwrap();
    let common = git::repository(&worktree).await.unwrap().common;
    let confined = |writable: Vec<PathBuf>, line: String| {
        let confinement = crate::platform::Confinement {
            writable,
            ..crate::platform::Confinement::around(&worktree, &worktree, None)
        };
        let worktree = worktree.clone();
        async move {
            let mut command = crate::platform::shell(&line, Some(&confinement)).unwrap();
            command.current_dir(&worktree);
            command.output().await.unwrap().status.success()
        }
    };
    let commit =
        "git add --all && git -c user.email=t@e.invalid -c user.name=t commit -qm b".to_owned();
    assert!(!confined(vec![worktree.clone()], commit.clone()).await);
    let mut open = vec![worktree.clone()];
    open.extend(there.git_writable().iter().cloned());
    assert!(confined(open.clone(), commit).await, "{open:?}");
    let hook = format!("echo x > '{}'", common.join("hooks/pre-commit").display());
    assert!(!confined(open, hook).await);
    assert!(
        work::status(&made)
            .await
            .unwrap()
            .contains("1 commit ahead")
    );
}

/// The yes was for a merge into `main`: a user who switched branch while it waited gets
/// no merge into the other one.
#[tokio::test]
async fn a_merge_is_checked_again_after_the_yes() {
    let (state, home, repo) = repository("branch-recheck").await;
    let made = work::start(&state, "chat", "chat", &home, Some("w"))
        .await
        .unwrap();
    commit_in(Path::new(&made.dir), "b.txt", "b\n");
    let switch = repo.clone();
    let (context, asked) = answering_after(&state, &home, Decision::Once, move || {
        run(&switch, &["switch", "-q", "-c", "other"]);
    });
    let error = work::merge(&context, &state, &made).await.err().unwrap();
    assert!(error.to_string().contains("is on other now"), "{error}");
    assert_eq!(asked.lock().len(), 1);
    assert!(!repo.join("b.txt").exists(), "merged into another branch");
    assert!(!git::merging(&repo).await);
    assert!(Path::new(&made.dir).is_dir());
    assert!(state.thread_worktree("chat").await.unwrap().is_some());
}

/// An agent asked with `isolated` from a thread on its own branch: the agent commits on
/// a branch made from the thread's, which the thread merges with an ordinary yes, its
/// own branch not before. An agent asked without `isolated` works in the thread's
/// worktree with the project's lookups. A record whose base is gone is forgotten.
#[tokio::test]
async fn an_agents_branch_is_merged_into_the_branch_it_came_from_first() {
    let (state, home, repo) = repository("branch-agent").await;
    let parent = work::start(&state, "chat", "chat", &home, Some("lead"))
        .await
        .unwrap();
    let home = Arc::new(home);
    let there = follow::scope_for(&state, "chat", &home, &home).await;
    state
        .create_agent_thread("helper", "chat", "general", "helper")
        .await
        .unwrap();
    let child = work::start(&state, "helper", "chat", &there, Some("helper"))
        .await
        .unwrap();
    assert_eq!(
        (child.base.as_str(), child.worktree_of.as_str()),
        ("agent/lead", parent.dir.as_str())
    );
    assert_eq!(child.project, repo.to_str().unwrap(), "the saved project");
    commit_in(Path::new(&child.dir), "b.txt", "b\n");

    let (context, asked) = answering(&state, &there, Decision::Once);
    let error = work::merge(&context, &state, &parent).await.err().unwrap();
    assert!(error.to_string().contains("agent/helper"), "{error}");
    let error = work::abandon(&context, &state, &parent)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("agent/helper"), "{error}");
    assert!(asked.lock().is_empty());

    let found = work::find(&state, "chat", Some("helper")).await.unwrap();
    let work::Done::Finished(text) = work::merge(&context, &state, &found).await.unwrap() else {
        panic!("the merge waits");
    };
    assert!(text.contains("into agent/lead"), "{text}");
    // Into an agent's branch: the mode's ordinary request, with a yes for the session.
    let asked = asked.lock().clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].session.is_some());
    assert!(Path::new(&parent.dir).join("b.txt").is_file());
    assert!(!repo.join("b.txt").exists());

    // An agent asked without `isolated` gets the worktree as its folder.
    state.create_thread("plain", "test").await.unwrap();
    let plain = Arc::new(ToolScope::new(there.cwd(), home.workspace()).unwrap());
    let scope = follow::scope_for(&state, "plain", &plain, &plain).await;
    assert_eq!(scope.cwd(), there.cwd());
    assert_eq!(
        scope.project_dir(),
        repo,
        "memory and variables of the project"
    );
    assert!(!scope.git_writable().is_empty());

    // Once the branch it came from is gone, only `abandon` clears the record.
    state.create_thread("late", "test").await.unwrap();
    let late = work::start(&state, "late", "late", &there, Some("late"))
        .await
        .unwrap();
    std::fs::write(Path::new(&late.dir).join("c.txt"), "c\n").unwrap();
    let (late_context, _) = answering(&state, &there, Decision::Once);
    let late_context = ToolContext {
        thread_id: "late".into(),
        ..late_context
    };
    let work::Done::Finished(_) = work::merge(&context, &state, &parent).await.unwrap() else {
        panic!("the merge waits");
    };
    assert!(repo.join("b.txt").is_file());
    assert!(!git::names(&repo).await.unwrap().contains(&parent.branch));
    let error = work::status(&late).await.err().unwrap();
    assert!(error.to_string().contains("abandon"), "{error}");
    assert!(work::merge(&late_context, &state, &late).await.is_err());
    let work::Done::Finished(text) = work::abandon(&late_context, &state, &late).await.unwrap()
    else {
        panic!("not forgotten");
    };
    assert!(text.contains("left as it is"), "{text}");
    assert_eq!(state.thread_worktree("late").await.unwrap(), None);
    assert!(Path::new(&late.dir).join("c.txt").is_file(), "nothing lost");
    assert!(git::names(&repo).await.unwrap().contains(&late.branch));
}
