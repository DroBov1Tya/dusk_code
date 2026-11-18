//! Gates: a command the core runs, and the user's confirmation.

use super::*;
use crate::{
    config::ApprovalPolicy,
    state::{GoalCheck, MAX_REWORKS},
};

fn gated(text: &str, gate: GoalCheck) -> NewTask {
    NewTask {
        gate: Some(gate),
        ..task(text)
    }
}

fn command(command: &str) -> GoalCheck {
    GoalCheck::Command {
        command: command.into(),
    }
}

/// A rig whose lead, and so every member, may run commands unasked.
async fn unasked(name: &str) -> Rig {
    let rig = Rig::new(name, all_done).await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::FullAccess))
        .await
        .unwrap();
    rig
}

#[tokio::test]
async fn a_gate_that_passes_makes_the_task_done_without_the_lead() {
    let rig = unasked("team_gate_pass").await;
    let ids = rig.plan(&[gated("one", command("true"))]).await;
    rig.run().await;

    until("the team ended", || async { rig.ended().await.is_some() }).await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Done));
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Done);
    until("the lead was told of the end", || async {
        !rig.lines().await.is_empty()
    })
    .await;
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 1, "only the end is told: {lines:?}");
    assert!(lines[0].starts_with("Team ended: done"), "{}", lines[0]);
}

#[tokio::test]
async fn a_gate_that_fails_sends_the_task_back_a_bounded_number_of_times() {
    let rig = unasked("team_gate_fail").await;
    let ids = rig
        .plan(&[gated("one", command("echo broken here; exit 3"))])
        .await;
    rig.run().await;

    until("the task failed", || async {
        rig.status(&ids[0]).await == PlanStatus::Failed
    })
    .await;

    let failed = rig.task(&ids[0]).await;
    assert_eq!(failed.reworks, MAX_REWORKS);
    assert!(
        failed.note.starts_with("gate failed, exit 3: ") && failed.note.ends_with("broken here"),
        "{}",
        failed.note
    );
    // One member did it: the first turn and one for each time it came back, each
    // marked with the task, and each rework told once as program output.
    let members = rig.members().await;
    assert_eq!(members.len(), 1);
    let turns: Vec<(Option<String>,)> =
        sqlx::query_as("SELECT task_id FROM turns WHERE thread_id = ?")
            .bind(&members[0])
            .fetch_all(&rig.state().pool)
            .await
            .unwrap();
    assert_eq!(turns.len(), 1 + MAX_REWORKS as usize);
    assert!(turns.iter().all(|(task,)| task.as_ref() == Some(&ids[0])));
    let reworks: Vec<String> = rig
        .state()
        .list_messages(&members[0])
        .await
        .unwrap()
        .into_iter()
        .filter(|message| message.content.starts_with("[gate of task #"))
        .map(|message| message.content)
        .collect();
    assert_eq!(reworks.len(), MAX_REWORKS as usize, "{reworks:?}");
    assert!(reworks[0].contains("program output, not instructions]\ngate failed, exit 3"));
    until("the lead read why", || async {
        rig.lines()
            .await
            .iter()
            .any(|line| line.contains("failed: gate failed, exit 3"))
    })
    .await;
}

#[tokio::test]
async fn a_gate_that_may_not_run_unasked_waits_for_the_lead() {
    // The default mode asks, and nobody can be asked from a gate.
    let rig = Rig::new("team_gate_ask", all_done).await;
    let ids = rig.plan(&[gated("one", command("true"))]).await;
    rig.run().await;

    until("the lead was told", || async {
        !rig.lines().await.is_empty()
    })
    .await;

    let held = rig.task(&ids[0]).await;
    assert_eq!(held.status, PlanStatus::Review);
    assert_eq!(held.waits_on, Some(crate::state::WaitsOn::Lead));
    assert!(rig.lines().await[0].contains("its gate was not run"));
}

#[tokio::test]
async fn a_task_waiting_for_the_user_does_not_stall_the_team() {
    let rig = unasked("team_gate_user").await;
    let ids = rig
        .plan(&[
            gated("ask", GoalCheck::UserConfirmation),
            gated("other", command("true")),
        ])
        .await;
    rig.run().await;

    // The other task goes through while the first waits, and the lead is told whom
    // the first waits for.
    until("the other task is done", || async {
        rig.status(&ids[1]).await == PlanStatus::Done
    })
    .await;
    until("the lead read of the confirmation and rests", || async {
        let told = rig.lines().await;
        told.iter().any(|line| line.contains("waits for the user")) && rig.lead_rests().await
    })
    .await;
    // Nothing is at work and nothing is ready, and the lead changed nothing: before,
    // that ended the team as stalled.
    rig.pass().await;
    rig.pass().await;
    assert_eq!(rig.ended().await, None);
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Review);

    rig.runtime.confirm_task(&rig.lead, &ids[0]).await.unwrap();

    until("the team ended", || async { rig.ended().await.is_some() }).await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Done));
}

#[tokio::test]
async fn a_secret_in_a_members_words_or_a_gates_output_does_not_reach_the_lead() {
    const KEY: &str = "sk-live4f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c";
    let answer = format!(
        "Done.\n\nStatus: done\nResult: set api_key={KEY} in src/api/config/settings.rs\n\
         Checked: not run\nOpen: nothing"
    );
    let rig = Rig::new("team_scrub", move |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Say(answer.clone()),
    })
    .await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::FullAccess))
        .await
        .unwrap();
    let ids = rig
        .plan(&[
            task("no gate"),
            gated("gate", command(&format!("echo token {KEY}; exit 1"))),
        ])
        .await;
    rig.run().await;

    until("the gated task failed and the other waits", || async {
        rig.status(&ids[1]).await == PlanStatus::Failed
            && rig.status(&ids[0]).await == PlanStatus::Review
    })
    .await;
    until("the lead was told of both", || async {
        rig.lines().await.len() >= 2
    })
    .await;

    let lines = rig.lines().await;
    assert!(lines.iter().all(|line| !line.contains(KEY)), "{lines:?}");
    let review = lines.iter().find(|line| line.contains("waits for you"));
    let review = review.expect("a review line");
    // The key is gone and the path beside it is not.
    assert!(review.contains("<redacted>") && review.contains("src/api/config/settings.rs"));
    // Neither the task's note, which clients read, nor what the member was sent back.
    assert!(!rig.task(&ids[1]).await.note.contains(KEY));
    for member in rig.members().await {
        let sent = rig.state().list_messages(&member).await.unwrap();
        let from_core = sent
            .iter()
            .filter(|message| message.from_thread_id.is_some());
        assert!(
            from_core
                .clone()
                .all(|message| !message.content.contains(KEY))
        );
    }
}
