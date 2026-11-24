//! What a run used, the check before a claim or a step, and a damaged thread tree.

use super::*;

#[tokio::test]
async fn used_counts_the_tree_since_the_start_and_until_the_end() {
    let state = chats().await;
    let tasks = add(&state, 2).await;
    state.start_team(LEAD, plain(), START).await.unwrap();
    claim(&state, &tasks[0], "m0").await;
    for (thread, purpose, tokens, at) in [
        (LEAD, "answer", (1, 1), START - 1),
        (LEAD, "answer", (10, 5), START),
        ("m0", "answer", (100, 50), START + 1_000),
        ("sub", "answer", (1_000, 500), START + 2_000),
        (LEAD, "compact", (7, 3), START + 3_000),
        ("m0", "title", (20, 2), START + 3_000),
        ("other", "answer", (5_000, 5_000), START + 1_000),
        ("other-m", "answer", (9_000, 1), START + 1_000),
    ] {
        usage(&state, thread, purpose, tokens, at).await;
    }
    let expected = TeamUsed {
        at_once: 1,
        steps: 3,
        tokens: 15 + 150 + 1_500 + 10 + 22,
        minutes: 5,
    };
    let now = START + 5 * MINUTE + 30_000;
    for thread in [LEAD, "m0", "sub"] {
        assert_eq!(state.team_used(thread, now).await.unwrap(), expected);
    }
    let team = state.team(LEAD, now).await.unwrap().unwrap();
    assert_eq!((team.used, team.thread_id.as_str()), (expected, LEAD));
    assert_eq!(team.members.len(), 1);
    let member = &team.members[0];
    assert_eq!(
        (member.thread_id.as_str(), member.name.as_str()),
        ("m0", "m0")
    );
    assert_eq!(
        (member.task_id.as_str(), member.model.as_deref()),
        (tasks[0].as_str(), Some("model-a"))
    );

    // A call after the end is not the run's; the time stops at the end.
    state
        .end_team(LEAD, TeamEnd::Stopped, START + 6 * MINUTE)
        .await
        .unwrap();
    usage(&state, LEAD, "answer", (50, 50), START + 7 * MINUTE).await;
    let after = state.team_used(LEAD, START + 60 * MINUTE).await.unwrap();
    assert_eq!(
        after,
        TeamUsed {
            at_once: 0,
            minutes: 6,
            ..expected
        }
    );

    // A second run counts from its own start.
    state
        .start_team(LEAD, plain(), START + 8 * MINUTE)
        .await
        .unwrap();
    let fresh = state.team_used(LEAD, START + 8 * MINUTE).await.unwrap();
    assert_eq!(fresh, TeamUsed::default());
}

/// A fresh chat whose team runs `at_once` members at once from `START`, with one task.
async fn running(at_once: u32) -> (State, String) {
    let state = chats().await;
    let task = add(&state, 1).await.remove(0);
    state.start_team(LEAD, at_once, START).await.unwrap();
    (state, task)
}

async fn check(state: &State) -> (TeamCheck, TeamCheck) {
    (
        state.team_check("m0", TeamAsk::Claim).await.unwrap(),
        state.team_check("m0", TeamAsk::Step).await.unwrap(),
    )
}

/// Members at once are the only bound of a running team: what it spent never stops it.
#[tokio::test]
async fn the_check_counts_members_at_once_and_nothing_spent() {
    use TeamCheck::{Full, Go};
    let (state, task) = running(1).await;
    assert_eq!(check(&state).await, (Go, Go));
    claim(&state, &task, "m0").await;
    assert_eq!(check(&state).await, (Full, Go), "members at once");
    for at in 0..50 {
        usage(&state, "m0", "answer", (100_000, 100_000), START + at).await;
    }
    assert_eq!(
        check(&state).await,
        (Full, Go),
        "steps and tokens bound nothing"
    );

    let (state, _) = running(1).await;
    state
        .end_team(LEAD, TeamEnd::Quiet, START + 1)
        .await
        .unwrap();
    let ended = TeamCheck::Ended(TeamEnd::Quiet);
    assert_eq!(check(&state).await, (ended, ended));
}

#[tokio::test]
async fn parent_links_that_loop_end_every_walk() {
    let state = chats().await;
    state.create_thread("x", "test").await.unwrap();
    state.create_thread("y", "test").await.unwrap();
    sqlx::raw_sql(
        "UPDATE threads SET parent_thread_id = 'y' WHERE id = 'x';
         UPDATE threads SET parent_thread_id = 'x' WHERE id = 'y';",
    )
    .execute(&state.pool)
    .await
    .expect("the schema lets a loop be written");
    usage(&state, "x", "answer", (1, 1), START).await;
    usage(&state, "y", "answer", (1, 1), START).await;

    let walks = async {
        // A thread in a loop has no chat, so it reaches no team.
        assert!(matches!(
            state.team_run("x").await,
            Err(TeamError::NoSuchThread(_))
        ));
        assert!(matches!(
            state.start_team("y", plain(), START).await,
            Err(TeamError::NoSuchThread(_))
        ));
        // The downward walk from a thread in the loop ends too.
        let run = TeamRun {
            thread_id: "x".to_owned(),
            status: TeamStatus::Running,
            at_once: plain(),
            started_at: START,
            ended_at: None,
            worktrees: false,
            base_commit: None,
            goal: String::new(),
        };
        let mut transaction = state.pool.begin().await.unwrap();
        used::used(&mut transaction, &run, START).await.unwrap()
    };
    let counted = tokio::time::timeout(Duration::from_secs(10), walks)
        .await
        .expect("the walks end");
    assert_eq!((counted.steps, counted.tokens), (2, 4));
}
