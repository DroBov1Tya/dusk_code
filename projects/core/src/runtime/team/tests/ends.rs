//! How a team ends short of done: stopped by the lead or the user, stalled, or quiet.
//! How a stop or the quiet cap winds a team down first is in `winds`.

use super::*;

#[tokio::test]
async fn stop_winds_down_then_cancels_the_members_and_does_not_wake_the_lead() {
    let rig = Rig::new("team_stop", hang).await;
    let ids = rig.plan(&[task("one"), task("two")]).await;
    assert_eq!(rig.run().await, 2);
    until("both members work", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
            && rig.status(&ids[1]).await == PlanStatus::Running
    })
    .await;

    rig.runner().stop(&rig.lead).await.unwrap();

    // It winds down: the members are asked to hand over, nothing is given out.
    assert_eq!(rig.ended().await, None);
    assert_eq!(rig.pass().await, 0);
    // Members that never answer keep it waiting until the grace ran out.
    rig.pass_after(GRACE_MS).await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    for id in &ids {
        let task = rig.task(id).await;
        assert_eq!(
            (task.status, task.owner_thread_id),
            (PlanStatus::Open, None)
        );
    }
    for member in rig.members().await {
        until("the member stopped", || async {
            !rig.turns(&member).await.1
        })
        .await;
    }
    // Stopped again, it is already so; a pass gives nothing out.
    rig.runner().stop(&rig.lead).await.unwrap();
    assert_eq!(rig.pass().await, 0);
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        lines[0],
        "Team winding down: 2 members are writing their hand-over."
    );
    assert!(lines[1].starts_with("Team ended: stopped. 0 of 2 tasks;"));
    assert_eq!(rig.state().count_turns(&rig.lead).await.unwrap(), 0);
    assert!(rig.state().has_pending(&rig.lead).await.unwrap());
}

#[tokio::test]
async fn a_user_who_stops_a_member_stops_the_team() {
    let rig = Rig::new("team_user_stop", hang).await;
    let ids = rig.plan(&[task("one")]).await;
    rig.run().await;
    until("the member works", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
    })
    .await;
    let member = rig.members().await.remove(0);

    rig.runtime.stop_agent(&member).await.unwrap();

    until("the team ended", || async { rig.ended().await.is_some() }).await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    // Nothing started the task again.
    assert_eq!(rig.pass().await, 0);
    assert_eq!(rig.members().await.len(), 1);
    assert_eq!(rig.task(&ids[0]).await.attempts, 1);
}

#[tokio::test]
async fn nothing_ready_and_a_lead_that_changes_nothing_ends_the_team_as_stalled() {
    let rig = Rig::new("team_stall", all_done).await;
    let ids = rig
        .plan(&[
            NewTask {
                key: Some("a".into()),
                ..task("given up")
            },
            NewTask {
                after: vec!["a".into()],
                ..task("behind it")
            },
        ])
        .await;
    rig.state()
        .fail_task(&rig.lead, &ids[0], "no")
        .await
        .unwrap();

    assert_eq!(rig.run().await, 0);
    // The line is written once, however often the dispatcher looks.
    rig.pass().await;
    rig.pass().await;
    assert_eq!(rig.ended().await, None, "the lead has not answered yet");
    until("the lead answered", || async {
        rig.turns(&rig.lead).await == (1, false)
    })
    .await;
    // It names what holds the next wave: the failed task of the one before.
    let failed = crate::state::short_task_id(&ids[0]).to_owned();
    assert_eq!(
        rig.lines().await,
        [format!(
            "Nothing can start: 1 failed, 1 behind them; wave 2 waits for wave 1: #{failed} \
             failed."
        )]
    );

    rig.pass().await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Stalled));
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].starts_with("Team ended: stalled, nothing could start. 0 of 2 tasks;"));
    assert!(rig.members().await.is_empty());
}

#[tokio::test]
async fn a_lead_written_to_thirty_times_ends_the_team_as_quiet() {
    let rig = Rig::new("team_quiet", hang).await;
    let ids = rig.plan(&[task("one")]).await;
    rig.run().await;
    let member = rig.members().await.remove(0);
    // Thirty messages from agents since the user last wrote: the chat's cap.
    for n in 0..30 {
        let uid = format!("note-{n}");
        let stored = rig
            .state()
            .add_message_from(&rig.lead, &uid, "a note", Some(&member));
        stored.await.unwrap();
    }

    rig.pass().await;

    // It winds down first: the member at work is asked to hand over.
    assert_eq!(rig.ended().await, None);
    assert!(
        rig.uids(&member)
            .await
            .iter()
            .any(|uid| uid.contains(":wrap:"))
    );
    rig.pass_after(GRACE_MS).await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Quiet));
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Open);
    // The end waits for the user's next message; it does not wake the chat.
    let lines = rig.lines().await;
    assert!(lines.last().unwrap().starts_with("Team ended: quiet"));
    assert!(rig.state().has_pending(&rig.lead).await.unwrap());
}

/// A quick task whose member answers when let through, and a slow one that never does.
fn quick_and_slow(asked: &Asked) -> Out {
    match (asked.lead, asked.user.contains("slow")) {
        (true, _) => Out::Say("Noted.".into()),
        (false, true) => Out::Hang,
        (false, false) => Out::Held(DONE.into()),
    }
}

#[tokio::test]
async fn a_member_takes_no_step_once_its_team_has_ended() {
    let rig = Rig::new("team_no_step", quick_and_slow).await;
    let ids = rig.plan(&[task("quick"), task("slow")]).await;
    assert_eq!(rig.run().await, 2);
    until("both members work", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
            && rig.status(&ids[1]).await == PlanStatus::Running
    })
    .await;
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
