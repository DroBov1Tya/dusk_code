//! Gates: a command the core runs, and the user's confirmation.

use super::*;
use crate::{
    config::ApprovalPolicy,
    state::{GoalCheck, MAX_REWORKS, TaskEdit, WaitsOn},
    tools::{Decision, GatesYes, Progress},
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

/// A client that answers every approval with yes once, and keeps the command lines of
/// each request it was shown.
fn yes_once(rig: &Rig) -> (Progress, Arc<Mutex<Vec<Vec<String>>>>) {
    let (events, mut seen) = tokio::sync::mpsc::unbounded_channel();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&asked);
    let approvals = Arc::clone(&rig.launcher().approvals);
    tokio::spawn(async move {
        while let Some(event) = seen.recv().await {
            let ThreadEvent::Approval(request) = event else {
                continue;
            };
            let lines = request.steps.iter().map(|step| match &step.request {
                crate::tools::Request::Command { command } => command.clone(),
                other => other.what(),
            });
            kept.lock().unwrap().push(lines.collect());
            approvals.answer(request.id, Decision::Once);
        }
    });
    let progress = Progress {
        call_id: "run".into(),
        events,
    };
    (progress, asked)
}

#[tokio::test]
async fn one_yes_at_run_covers_exactly_the_listed_gates() {
    // Members hand in only when let through, so the test edits the plan between.
    let rig = Rig::new("team_gate_yes", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Held(DONE.into()),
    })
    .await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::Ask))
        .await
        .unwrap();
    let first = rig.plan(&[gated("one", command("true"))]).await;
    let second = rig
        .plan(&[NewTask {
            after: first.clone(),
            ..gated("two", command("true"))
        }])
        .await;
    let (progress, asked) = yes_once(&rig);
    let started = rig
        .runner()
        .run(&rig.lead, Some(progress.clone()))
        .await
        .unwrap();
    assert_eq!(started.gates, GatesYes::Allowed);
    // One request, each distinct command once.
    assert_eq!(*asked.lock().unwrap(), [vec!["true".to_owned()]]);

    // A gate changed after the yes is not covered by it.
    let edit = TaskEdit {
        gate: Some(Some(command("true && true"))),
        ..TaskEdit::default()
    };
    rig.state()
        .edit_plan_task(&rig.lead, &second[0], &edit)
        .await
        .unwrap();
    rig.let_through();
    until("the allowed gate passed the first task", || async {
        rig.status(&first[0]).await == PlanStatus::Done
    })
    .await;
    rig.let_through();
    until("the changed gate was held for the lead", || async {
        rig.task(&second[0]).await.waits_on == Some(WaitsOn::Lead)
    })
    .await;
    let held = rig.task(&second[0]).await;
    assert_eq!(held.status, PlanStatus::Review);
    assert!(held.note.contains("its gate was not run"), "{}", held.note);

    // The next run asks for the changed line alone.
    let again = rig.runner().run(&rig.lead, Some(progress)).await.unwrap();
    assert_eq!(again.gates, GatesYes::Allowed);
    assert_eq!(
        asked.lock().unwrap().last().cloned(),
        Some(vec!["true && true".to_owned()])
    );
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

#[tokio::test]
async fn a_change_outside_the_tasks_files_holds_it_for_the_lead() {
    // The member writes its file, one more, and build output the project ignores.
    let rig = Rig::new("team_outside_files", |asked| match (asked.lead, &asked.result) {
        (true, _) => Out::Say("Noted.".into()),
        (false, None) => Out::Call(
            "run_command",
            serde_json::json!({ "command":
                "echo a > src/a.rs && echo b > notes.md && mkdir -p target && echo o > target/x.o"
            }),
        ),
        (false, Some(_)) => Out::Say(DONE.into()),
    })
    .await;
    std::fs::write(rig.folder.join(".gitignore"), "target/\n").unwrap();
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::FullAccess))
        .await
        .unwrap();
    let ids = rig
        .plan(&[NewTask {
            files: crate::state::TaskFiles::new(["src/a.rs"]).unwrap(),
            ..gated("one", command("true"))
        }])
        .await;
    rig.run().await;

    until("the task was held for the lead", || async {
        rig.task(&ids[0]).await.waits_on == Some(WaitsOn::Lead)
    })
    .await;
    let held = rig.task(&ids[0]).await;
    assert_eq!(held.status, PlanStatus::Review, "its gate did not pass it");
    assert!(
        held.note.ends_with("changed outside its files: notes.md"),
        "{}",
        held.note
    );
    // The lead accepts it, as the store lets it for a held task.
    rig.state()
        .pass_task(&rig.lead, &ids[0], crate::state::PassedBy::Lead)
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn stopping_the_team_kills_its_running_gate() {
    let rig = unasked("team_gate_stop").await;
    // The gate's own child, in the gate's process group, says who it is and waits.
    let ids = rig
        .plan(&[gated("one", command("sleep 60 & echo $! > gate.pid; wait"))])
        .await;
    rig.run().await;
    let pid_file = rig.folder.join("gate.pid");
    until("the gate runs", || async {
        std::fs::read_to_string(&pid_file).is_ok_and(|pid| pid.ends_with('\n'))
    })
    .await;
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let pid = nix::unistd::Pid::from_raw(pid);
    assert!(nix::sys::signal::kill(pid, None).is_ok(), "the gate runs");

    rig.launcher().team_stop(&rig.lead).await.unwrap();

    until("the gate's child is gone", || async {
        nix::sys::signal::kill(pid, None).is_err()
    })
    .await;
    // A gate is a check: its task waits in review for the next run to run it again.
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Review);
    assert!(rig.launcher().team.gates.lock().is_empty());
}
