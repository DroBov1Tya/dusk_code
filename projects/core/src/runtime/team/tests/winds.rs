//! How a team winds down: a stop, the quiet cap or a failing provider gives out nothing
//! new, the core asks each member at work to hand over, a task not finished goes back to
//! open with its member's footer, and losses that are not the member's are not counted.

use super::*;
use crate::providers::ProviderErrorKind;

/// A member's footer when it stops half way.
const PARTIAL: &str = "Half way.\n\nStatus: partial\nResult: the parser reads headers\nChecked: \
                       cargo test parser passed\nNot checked: the body\nOpen: parse the body";

/// The first member answers `PARTIAL` once let through; one whose card carries a
/// hand-over finishes.
fn hands_over(asked: &Asked) -> Out {
    match (asked.lead, asked.user.contains("Before you")) {
        (true, _) => Out::Say("Noted.".into()),
        (false, true) => Out::Say(DONE.into()),
        (false, false) => Out::Held(PARTIAL.into()),
    }
}

/// The settings with no retry of a failed call, so a provider's failure fails the turn.
const ONCE: &str = "mode: TEAM\nlimits:\n  plan_graph: true\n  team_review: false\n  \
                    max_attempts: 1\n";

#[tokio::test]
async fn a_stopped_member_hands_over_and_the_next_card_carries_it() {
    let rig = Rig::new("team_wind_hand_over", hands_over).await;
    let id = rig.plan(&[task("parse the file")]).await.remove(0);
    assert_eq!(rig.run().await, 1);
    until("the member works", || async {
        rig.status(&id).await == PlanStatus::Running
    })
    .await;
    let first = rig.members().await.remove(0);

    rig.runner().stop(&rig.lead).await.unwrap();

    // The core, not the lead, asked it to stop; nothing is given out meanwhile.
    let started = rig.state().team_run(&rig.lead).await.unwrap().unwrap();
    let wrap = format!("team:{}:{}:wrap:{first}", rig.lead, started.started_at);
    assert!(rig.uids(&first).await.contains(&wrap));
    assert!(started.winding.is_some());
    assert_eq!(rig.ended().await, None);
    rig.let_through();
    until("the task came back", || async {
        rig.status(&id).await == PlanStatus::Open
    })
    .await;
    let back = rig.task(&id).await;
    assert_eq!(back.attempts, 0, "a hand-over is no lost attempt");
    assert_eq!(back.owner_thread_id, None);
    assert!(
        back.result.starts_with("Status: partial"),
        "{}",
        back.result
    );
    // Nobody is at work: the next pass ends it, and the lead was never woken.
    rig.pass().await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    assert_eq!(rig.state().count_turns(&rig.lead).await.unwrap(), 0);

    // The next run gives it to a new member, whose card carries the hand-over.
    assert_eq!(rig.run().await, 1);
    until("the second member handed in", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;
    let members = rig.members().await;
    assert_eq!(members.len(), 2);
    let messages = rig.state().list_messages(&members[1]).await.unwrap();
    let card = &messages
        .iter()
        .find(|message| message.content.starts_with("Task #"))
        .unwrap()
        .content;
    assert!(
        card.contains(
            "Before you (last attempt's hand-over): Status: partial | Result: the parser reads \
             headers | Checked: cargo test parser passed | Not checked: the body | Open: parse \
             the body"
        ),
        "{card}"
    );
    assert_eq!(rig.task(&id).await.attempts, 1);
}

#[tokio::test]
async fn a_member_that_finishes_while_its_team_winds_down_hands_in() {
    let rig = Rig::new("team_wind_done", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Held(DONE.into()),
    })
    .await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    rig.run().await;
    until("the member works", || async {
        rig.status(&id).await == PlanStatus::Running
    })
    .await;

    rig.runner().stop(&rig.lead).await.unwrap();
    rig.let_through();

    until("the task waits for the lead", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;
    rig.pass().await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    // Its line waits for the lead's next request; the stop woke nobody.
    assert!(
        rig.lines()
            .await
            .iter()
            .any(|line| line.contains("waits for you"))
    );
    assert_eq!(rig.state().count_turns(&rig.lead).await.unwrap(), 0);
}

#[tokio::test]
async fn a_member_steps_only_within_the_grace_and_a_second_stop_ends_the_team_at_once() {
    let rig = Rig::new("team_wind_twice", hang).await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    rig.run().await;
    until("the member works", || async {
        rig.status(&id).await == PlanStatus::Running
    })
    .await;
    let member = rig.members().await.remove(0);
    rig.runner().stop(&rig.lead).await.unwrap();
    assert_eq!(rig.ended().await, None);
    assert_eq!(
        rig.lines().await,
        ["Team winding down: 1 member is writing its hand-over."]
    );
    // The lead cannot run it again while its members hand over.
    let refused = rig.try_run().await.unwrap_err().to_string();
    assert!(refused.contains("the team is stopping"), "{refused}");
    let config = rig.launcher().config.current();
    // Within the grace the member may still step, to write its hand-over; past it, not.
    super::super::may_step(rig.state(), &config, &member, 1)
        .await
        .unwrap();
    sqlx::query("UPDATE teams SET winding_since = winding_since - ? WHERE thread_id = ?")
        .bind(GRACE_MS)
        .bind(&rig.lead)
        .execute(&rig.state().pool)
        .await
        .unwrap();
    let late = super::super::may_step(rig.state(), &config, &member, 1).await;
    assert!(
        late.unwrap_err()
            .to_string()
            .contains("the team was stopped")
    );

    rig.runner().stop(&rig.lead).await.unwrap();

    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    assert_eq!(rig.status(&id).await, PlanStatus::Open);
}

#[tokio::test]
async fn a_key_the_provider_refuses_is_no_lost_attempt_and_stops_the_team() {
    let rig = Rig::with("team_wind_auth", ONCE, |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Fail(ProviderErrorKind::Auth),
    })
    .await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    rig.run().await;

    // The end is stored before its line is written.
    until("the lead was told", || async {
        rig.lines()
            .await
            .iter()
            .any(|line| line.starts_with("Team ended"))
    })
    .await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    let task = rig.task(&id).await;
    assert_eq!((task.status, task.attempts), (PlanStatus::Open, 0));
    let lines = rig.lines().await;
    assert!(
        lines.last().unwrap().starts_with(
            "Team ended: stopped, the provider kept failing the members' turns. 0 of 1 tasks;"
        ),
        "{lines:?}"
    );
}

#[tokio::test]
async fn three_provider_failures_in_a_row_stop_the_team_and_fail_no_task() {
    let rig = Rig::with("team_wind_infra", ONCE, |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Fail(ProviderErrorKind::Interrupted),
    })
    .await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    rig.run().await;

    until("the team ended", || async { rig.ended().await.is_some() }).await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Stopped));
    // Three members lost it, each to the provider: none of it counted.
    assert_eq!(rig.members().await.len(), 3);
    let task = rig.task(&id).await;
    assert_eq!((task.status, task.attempts), (PlanStatus::Open, 0));
}
