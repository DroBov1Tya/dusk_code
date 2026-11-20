//! Each budget ends the team with its own reason and stops the members still at work.

use super::*;

/// A quick task whose member answers when let through, and a slow one that never does.
fn quick_and_slow(asked: &Asked) -> Out {
    match (asked.lead, asked.user.contains("slow")) {
        (true, _) => Out::Say("Noted.".into()),
        (false, true) => Out::Hang,
        (false, false) => Out::Held(DONE.into()),
    }
}

/// Runs both tasks with the budget `change` makes; answers their ids once both members
/// are at work.
async fn both_at_work(rig: &Rig, change: impl FnOnce(TeamBudget) -> TeamBudget) -> Vec<String> {
    let ids = rig.plan(&[task("quick"), task("slow")]).await;
    assert_eq!(rig.run_with(change).await.unwrap(), 2);
    until("both members work", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
            && rig.status(&ids[1]).await == PlanStatus::Running
    })
    .await;
    ids
}

/// The team ended with `why`: the slow member was stopped and its task is open again.
async fn ended_with(rig: &Rig, ids: &[String], why: TeamEnd) {
    until("the team ended", || async { rig.ended().await.is_some() }).await;
    assert_eq!(rig.ended().await, Some(why));
    let slow = rig.members().await.remove(1);
    until("the slow member stopped", || async {
        !rig.turns(&slow).await.1
    })
    .await;
    let task = rig.task(&ids[1]).await;
    assert_eq!(
        (task.status, task.owner_thread_id),
        (PlanStatus::Open, None)
    );
    // Returned by the end, not lost: the attempt is not counted against the task twice.
    assert_eq!(task.attempts, 1);
    assert_eq!(rig.pass().await, 0);
    let lines = rig.lines().await;
    let ends: Vec<_> = lines
        .iter()
        .filter(|line| line.contains("Team ended:"))
        .collect();
    assert_eq!(ends.len(), 1, "{lines:?}");
}

#[tokio::test]
async fn spent_steps_end_the_team() {
    let rig = Rig::new("team_steps", quick_and_slow).await;
    let one_step = |budget: TeamBudget| {
        TeamBudget::new(budget.at_once(), 1, budget.tokens(), budget.minutes()).unwrap()
    };
    let ids = both_at_work(&rig, one_step).await;
    rig.let_through();
    ended_with(&rig, &ids, TeamEnd::Steps).await;
    // The quick member's one step was taken and handed in before the end.
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Review);
}

#[tokio::test]
async fn spent_tokens_end_the_team() {
    let rig = Rig::new("team_tokens", quick_and_slow).await;
    let few_tokens = |budget: TeamBudget| {
        TeamBudget::new(budget.at_once(), budget.steps(), 50, budget.minutes()).unwrap()
    };
    let ids = both_at_work(&rig, few_tokens).await;
    rig.let_through();
    ended_with(&rig, &ids, TeamEnd::Tokens).await;
}

#[tokio::test]
async fn the_deadline_ends_the_team() {
    let rig = Rig::new("team_time", quick_and_slow).await;
    let five_minutes = |budget: TeamBudget| {
        TeamBudget::new(budget.at_once(), budget.steps(), budget.tokens(), 5).unwrap()
    };
    let ids = both_at_work(&rig, five_minutes).await;
    let run = rig.state().team_run(&rig.lead).await.unwrap().unwrap();

    let launcher = rig.launcher();
    launcher
        .team_pass_at(&rig.lead, run.deadline + 1)
        .await
        .unwrap();

    until("the team ended", || async { rig.ended().await.is_some() }).await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Time));
    for member in rig.members().await {
        until("the member stopped", || async {
            !rig.turns(&member).await.1
        })
        .await;
    }
    for id in &ids {
        assert_eq!(rig.status(id).await, PlanStatus::Open);
    }
}

#[tokio::test]
async fn a_member_takes_no_step_once_its_team_has_ended() {
    let rig = Rig::new("team_no_step", quick_and_slow).await;
    let ids = both_at_work(&rig, |budget| budget).await;
    // An end the store alone made, with no runner stopped, as a stop cut short leaves it.
    let now = crate::state::timestamp_ms().unwrap();
    rig.state()
        .end_team(&rig.lead, TeamEnd::Stopped, now)
        .await
        .unwrap();
    let quick = rig.members().await.remove(0);
    let config = rig.launcher().config.current();

    let refused = super::super::may_step(rig.state(), &config, &quick, 1).await;

    assert!(refused.unwrap_err().to_string().contains("holds no task"));
    // The lead and an agent that is no member are never asked.
    super::super::may_step(rig.state(), &config, &rig.lead, 0)
        .await
        .unwrap();
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Open);
}
