//! What a run used, the check before a claim or a step, and a damaged thread tree.

use super::*;

#[tokio::test]
async fn used_counts_the_tree_since_the_start_and_until_the_end() {
    let state = chats().await;
    let tasks = add(&state, 2).await;
    state
        .start_team(LEAD, plain(), ceilings(), START)
        .await
        .unwrap();
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
        .start_team(LEAD, plain(), ceilings(), START + 8 * MINUTE)
        .await
        .unwrap();
    let fresh = state.team_used(LEAD, START + 8 * MINUTE).await.unwrap();
    assert_eq!(fresh, TeamUsed::default());
}

/// A fresh chat whose team runs on `budget` from `START`, with one task to claim.
async fn running(budget: TeamBudget) -> (State, String) {
    let state = chats().await;
    let task = add(&state, 1).await.remove(0);
    state
        .start_team(LEAD, budget, ceilings(), START)
        .await
        .unwrap();
    (state, task)
}

async fn check(state: &State, now: i64) -> (TeamCheck, TeamCheck) {
    (
        state.team_check("m0", TeamAsk::Claim, now).await.unwrap(),
        state.team_check("m0", TeamAsk::Step, now).await.unwrap(),
    )
}

#[tokio::test]
async fn the_check_names_the_budget_that_is_spent() {
    use TeamCheck::{Full, Go};
    let small = budget(1, 2, 1_000, 5);

    let (state, task) = running(small).await;
    assert_eq!(check(&state, START).await, (Go, Go));
    claim(&state, &task, "m0").await;
    assert_eq!(check(&state, START).await, (Full, Go), "members at once");
    usage(&state, "m0", "answer", (1, 1), START + 1).await;
    assert_eq!(check(&state, START + 2).await, (Full, Go));
    usage(&state, "sub", "answer", (1, 1), START + 1).await;
    let steps = TeamCheck::Spent(Spent::Steps);
    assert_eq!(check(&state, START + 2).await, (steps, steps));

    let (state, _) = running(small).await;
    usage(&state, LEAD, "compact", (999, 1), START + 1).await;
    let tokens = TeamCheck::Spent(Spent::Tokens);
    assert_eq!(check(&state, START + 2).await, (tokens, tokens));

    let (state, _) = running(small).await;
    assert_eq!(check(&state, START + 5 * MINUTE - 1).await, (Go, Go));
    let time = TeamCheck::Spent(Spent::Time);
    assert_eq!(check(&state, START + 5 * MINUTE).await, (time, time));

    let (state, _) = running(small).await;
    state
        .end_team(LEAD, TeamEnd::Quiet, START + 1)
        .await
        .unwrap();
    let ended = TeamCheck::Ended(TeamEnd::Quiet);
    assert_eq!(check(&state, START + 2).await, (ended, ended));

    let ends: Vec<TeamEnd> = [Spent::Steps, Spent::Tokens, Spent::Time]
        .into_iter()
        .map(Spent::end)
        .collect();
    assert_eq!(ends, [TeamEnd::Steps, TeamEnd::Tokens, TeamEnd::Time]);
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
            state.start_team("y", plain(), ceilings(), START).await,
            Err(TeamError::NoSuchThread(_))
        ));
        // The downward walk from a thread in the loop ends too.
        let run = TeamRun {
            thread_id: "x".to_owned(),
            status: TeamStatus::Running,
            budget: plain(),
            started_at: START,
            deadline: START + MINUTE,
            ended_at: None,
            worktrees: false,
            base_commit: None,
        };
        let mut transaction = state.pool.begin().await.unwrap();
        used::used(&mut transaction, &run, START).await.unwrap()
    };
    let counted = tokio::time::timeout(Duration::from_secs(10), walks)
        .await
        .expect("the walks end");
    assert_eq!((counted.steps, counted.tokens), (2, 4));
}
