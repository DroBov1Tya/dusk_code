//! How a team ends short of done: stopped by the lead or the user, stalled, or quiet.

use super::*;

#[tokio::test]
async fn stop_cancels_the_members_and_does_not_wake_the_lead() {
    let rig = Rig::new("team_stop", hang).await;
    let ids = rig.plan(&[task("one"), task("two")]).await;
    assert_eq!(rig.run().await, 2);
    until("both members work", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
            && rig.status(&ids[1]).await == PlanStatus::Running
    })
    .await;

    rig.runner().stop(&rig.lead).await.unwrap();

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
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("Team ended: stopped. 0 of 2 tasks;"));
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
    assert_eq!(
        rig.lines().await,
        ["Nothing can start: 1 failed, 1 behind them."]
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

    assert_eq!(rig.ended().await, Some(TeamEnd::Quiet));
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Open);
    // The end waits for the user's next message; it does not wake the chat.
    let lines = rig.lines().await;
    assert!(lines.last().unwrap().starts_with("Team ended: quiet"));
    assert!(rig.state().has_pending(&rig.lead).await.unwrap());
}
