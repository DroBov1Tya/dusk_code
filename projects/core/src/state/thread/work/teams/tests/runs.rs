//! Starting and ending runs, the work an end gives back, and one chat's team kept from another's.

use super::*;

#[tokio::test]
async fn every_end_is_kept_and_a_later_run_starts_again() {
    let state = chats().await;
    assert!(matches!(
        state.end_team(LEAD, TeamEnd::Done, START).await,
        Err(TeamError::NoTeam)
    ));
    assert!(matches!(
        state.team_check(LEAD, TeamAsk::Claim).await,
        Err(TeamError::NoTeam)
    ));
    assert!(state.team(LEAD, START).await.unwrap().is_none());
    let ends = [
        TeamEnd::Done,
        TeamEnd::Steps,
        TeamEnd::Tokens,
        TeamEnd::Time,
        TeamEnd::Stalled,
        TeamEnd::Stopped,
        TeamEnd::Quiet,
    ];
    for (index, why) in ends.into_iter().enumerate() {
        let at = START + i64::try_from(index).unwrap() * 10 * MINUTE;
        let run = state.start_team(LEAD, plain(), at).await.unwrap();
        assert_eq!((run.status, run.started_at), (TeamStatus::Running, at));
        // Clients tell this run from the last one by when it started.
        let shown = state.team(LEAD, at).await.unwrap().unwrap();
        assert_eq!(shown.started_at, Some(at));
        assert!(matches!(
            state.start_team("m0", plain(), at + 1).await,
            Err(TeamError::Running)
        ));
        let (run, returned) = state.end_team(LEAD, why, at + 2).await.unwrap();
        assert!(returned.is_empty());
        assert_eq!(
            (run.status, run.ended_at),
            (TeamStatus::Ended(why), Some(at + 2))
        );
        let stored = state.team_run(LEAD).await.unwrap().unwrap();
        assert_eq!(stored, run);
        assert!(matches!(
            state.end_team(LEAD, TeamEnd::Stopped, at + 3).await,
            Err(TeamError::Ended(before)) if before == why
        ));
        assert!(matches!(
            state.set_team_at_once(LEAD, plain(), 4).await,
            Err(TeamError::Ended(before)) if before == why
        ));
        assert_eq!(
            state.team_check(LEAD, TeamAsk::Step).await.unwrap(),
            TeamCheck::Ended(why)
        );
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "one team a chat, its row reset by each run");
}

#[tokio::test]
async fn the_end_gives_work_back_and_a_run_waits_for_work_left() {
    let state = chats().await;
    let tasks = add(&state, 3).await;
    state.start_team(LEAD, plain(), START).await.unwrap();
    claim(&state, &tasks[0], "m0").await;
    claim(&state, &tasks[1], "m1").await;
    let turn_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at)
         VALUES (?, 'm1', 1, 'interrupted', 0, 0)",
    )
    .bind(&turn_id)
    .execute(&state.pool)
    .await
    .unwrap();
    state.start_task("m1", &tasks[1], &turn_id).await.unwrap();

    let (_, returned) = state
        .end_team(LEAD, TeamEnd::Stopped, START + 1)
        .await
        .unwrap();
    assert_eq!(returned.len(), 2);
    for task in state.plan(LEAD).await.unwrap() {
        assert_eq!(task.status, PlanStatus::Open);
        assert_eq!(task.owner_thread_id, None);
    }
    let attempts: Vec<u32> = state
        .plan(LEAD)
        .await
        .unwrap()
        .iter()
        .map(|task| task.attempts)
        .collect();
    assert_eq!(attempts, [1, 1, 0], "an end does not cost an attempt");

    // A crash after the end committed and before the work came back.
    state.start_team(LEAD, plain(), START + 2).await.unwrap();
    claim(&state, &tasks[2], "m0").await;
    sqlx::raw_sql("UPDATE teams SET status = 'ended', ended_why = 'stopped', ended_at = 10000003")
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(state.team_counts(LEAD).await.unwrap().at_work, 1);
    assert!(matches!(
        state.start_team(LEAD, plain(), START + 4).await,
        Err(TeamError::WorkLeft(1))
    ));
    state.return_work(LEAD).await.unwrap();
    state.start_team(LEAD, plain(), START + 5).await.unwrap();
}

#[tokio::test]
async fn a_chat_reaches_its_own_team_and_no_other() {
    let state = chats().await;
    let tasks = add(&state, 1).await;
    state.start_team(LEAD, plain(), START).await.unwrap();
    claim(&state, &tasks[0], "m0").await;

    for thread in ["other", "other-m"] {
        assert!(state.team_run(thread).await.unwrap().is_none());
        assert!(state.team(thread, START).await.unwrap().is_none());
        assert!(state.team_members(thread).await.unwrap().is_empty());
        assert_eq!(state.team_counts(thread).await.unwrap().total, 0);
        assert!(matches!(
            state.end_team(thread, TeamEnd::Stopped, START).await,
            Err(TeamError::NoTeam)
        ));
        assert!(matches!(
            state.set_team_at_once(thread, plain(), 4).await,
            Err(TeamError::NoTeam)
        ));
        assert!(matches!(
            state.team_check(thread, TeamAsk::Claim).await,
            Err(TeamError::NoTeam)
        ));
    }
    for thread in [LEAD, "m0", "sub"] {
        let run = state.team_run(thread).await.unwrap().unwrap();
        assert_eq!(run.thread_id, LEAD);
    }
    assert!(matches!(
        state.team_run("ghost").await,
        Err(TeamError::NoSuchThread(_))
    ));

    // The other chat's own team leaves this one as it is.
    let other = state.start_team("other-m", 1, START).await.unwrap();
    assert_eq!(other.thread_id, "other");
    state
        .end_team("other", TeamEnd::Stopped, START + 1)
        .await
        .unwrap();
    let lead = state.team_run(LEAD).await.unwrap().unwrap();
    assert_eq!((lead.status, lead.at_once), (TeamStatus::Running, plain()));
    assert_eq!(state.team_counts(LEAD).await.unwrap().at_work, 1);

    // A deleted chat takes its team with it.
    state.delete_thread(LEAD).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams WHERE thread_id = ?")
        .bind(LEAD)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn a_run_winds_down_once_keeps_why_after_its_end_and_the_next_run_forgets_it() {
    use super::super::{WindWhy, Winding};
    let state = chats().await;
    assert!(matches!(
        state.wind_team(LEAD, WindWhy::Stopped, START).await,
        Err(TeamError::NoTeam)
    ));
    state.start_team(LEAD, plain(), START).await.unwrap();

    assert!(
        state
            .wind_team(LEAD, WindWhy::Provider, START + 1)
            .await
            .unwrap()
    );
    // The first reason and time stand.
    assert!(
        !state
            .wind_team(LEAD, WindWhy::Stopped, START + 2)
            .await
            .unwrap()
    );
    let winding = Winding {
        since: START + 1,
        why: WindWhy::Provider,
    };
    assert_eq!(
        state.team_run(LEAD).await.unwrap().unwrap().winding,
        Some(winding)
    );
    for ask in [TeamAsk::Claim, TeamAsk::Step] {
        assert_eq!(
            state.team_check(LEAD, ask).await.unwrap(),
            TeamCheck::Winding(START + 1)
        );
    }
    let shown = state.team(LEAD, START + 3).await.unwrap().unwrap();
    assert_eq!(shown.winding_since, Some(START + 1));

    // Ended, it keeps why it wound down, and clients no longer see it winding.
    let (run, _) = state
        .end_team(LEAD, TeamEnd::Stopped, START + 4)
        .await
        .unwrap();
    assert_eq!(run.winding, Some(winding));
    let shown = state.team(LEAD, START + 5).await.unwrap().unwrap();
    assert_eq!(shown.winding_since, None);
    assert!(matches!(
        state.wind_team(LEAD, WindWhy::Stopped, START + 5).await,
        Err(TeamError::Ended(TeamEnd::Stopped))
    ));
    let again = state.start_team(LEAD, plain(), START + 6).await.unwrap();
    assert_eq!(again.winding, None);

    // The table refuses a time without a reason, or a reason it does not know.
    for set in [
        "UPDATE teams SET winding_since = 1 WHERE thread_id = ?",
        "UPDATE teams SET winding_why = 'stopped' WHERE thread_id = ?",
        "UPDATE teams SET winding_since = 1, winding_why = 'bored' WHERE thread_id = ?",
    ] {
        let wrote = sqlx::query(set).bind(LEAD).execute(&state.pool).await;
        assert!(wrote.is_err(), "{set}");
    }
}
