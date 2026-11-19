//! A plan from ready to done, and what stops a task on the way.

use serde_json::json;

use super::*;
use crate::state::{GoalCheck, LEASE_MS, TaskFiles, WaitsOn, timestamp_ms};

/// The id in `#1a2b3c4d waits for you`.
fn waiting_id(line: &str) -> Option<&str> {
    let before = line.split(" waits for you").next()?;
    let id = before.rsplit('#').next()?;
    (before.len() < line.len() && id.len() == 8).then_some(id)
}

/// A lead that writes a chain of three tasks, runs it, accepts what waits for it and
/// says a word to everything else.
fn chain_lead(asked: &Asked) -> Out {
    if let Some(result) = &asked.result {
        return match result.starts_with("added") {
            true => Out::Call("tasks", json!({ "action": "run" })),
            false => Out::Say("Waiting.".into()),
        };
    }
    if let Some(id) = waiting_id(&asked.user) {
        return Out::Call("tasks", json!({ "action": "review", "ids": [id] }));
    }
    if asked.user != "go" {
        return Out::Say("Noted.".into());
    }
    let tasks = json!([
        { "text": "first", "key": "a" },
        { "text": "second", "key": "b", "after": ["a"], "gate": "true" },
        { "text": "third", "after": ["b"] },
    ]);
    Out::Call("tasks", json!({ "action": "add", "tasks": tasks }))
}

#[tokio::test]
async fn a_chain_runs_to_done_and_wakes_the_lead_only_for_its_reviews() {
    let rig = Rig::new("team_chain", |asked| match asked.lead {
        true => chain_lead(asked),
        false => Out::Held(DONE.into()),
    })
    .await;
    // The user says no when `run` asks for the gate, so the gated task waits too.
    let approvals = Arc::clone(&rig.launcher().approvals);
    let events = Arc::clone(&rig.events);
    tokio::spawn(async move {
        loop {
            let asked: Vec<u64> = events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|event| match event {
                    ThreadEvent::Approval(request) => Some(request.id),
                    _ => None,
                })
                .collect();
            for id in asked {
                approvals.answer(id, crate::tools::Decision::Deny);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    rig.input.send("go").await.unwrap();
    // Each member answers while the lead rests, so every line starts a turn of its own.
    for reviews in 0..3 {
        until("the lead's turn ended with a member at work", || async {
            let counts = rig.state().team_counts(&rig.lead).await.unwrap();
            rig.turns(&rig.lead).await == (1 + reviews, false) && counts.at_work == 1
        })
        .await;
        rig.let_through();
    }
    until("the team ended", || async { rig.ended().await.is_some() }).await;
    until("the lead read of the end", || async {
        rig.turns(&rig.lead).await == (4, false) && rig.lead_rests().await
    })
    .await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Done));
    let plan = rig.state().plan(&rig.lead).await.unwrap();
    assert!(plan.iter().all(|task| task.status == PlanStatus::Done));
    assert_eq!(rig.members().await.len(), 3);
    // One turn for the plan, one for each review; the end joined the last review.
    assert_eq!(rig.state().count_turns(&rig.lead).await.unwrap(), 4);
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(lines[0].contains("waits for you: no gate. Result: it works now."));
    assert!(lines[1].contains("its gate was not run"), "{}", lines[1]);
    assert!(lines[3].starts_with("Team ended: done. 3 of 3 tasks;"));
    assert!(lines.iter().all(|line| line.len() <= 400));
    // Clients heard of every task and of the team.
    let events = rig.events.lock().unwrap();
    let heard = |wanted: fn(&ThreadEvent) -> bool| events.iter().filter(|e| wanted(e)).count();
    assert!(heard(|event| matches!(event, ThreadEvent::TaskChanged(_))) >= 9);
    assert!(heard(|event| matches!(event, ThreadEvent::Team(_))) >= 2);
}

#[tokio::test]
async fn no_more_members_work_at_once_than_the_budget_says() {
    let settings = format!("{ON}  team_at_once: 2\n");
    let rig = Rig::with("team_at_once", &settings, hang).await;
    let tasks: Vec<NewTask> = (0..5).map(|n| task(&format!("task {n}"))).collect();
    let ids = rig.plan(&tasks).await;

    assert_eq!(rig.run().await, 2);
    assert_eq!(rig.pass().await, 0);
    assert_eq!(rig.members().await.len(), 2);
    let counts = rig.state().team_counts(&rig.lead).await.unwrap();
    assert_eq!((counts.at_work, counts.ready), (2, 3));
    // Oldest first.
    assert_ne!(rig.status(&ids[0]).await, PlanStatus::Open);
    assert_ne!(rig.status(&ids[1]).await, PlanStatus::Open);
    assert_eq!(rig.status(&ids[2]).await, PlanStatus::Open);
}

#[tokio::test]
async fn the_lowest_phase_goes_first_and_run_twice_changes_nothing() {
    let settings = format!("{ON}  team_at_once: 2\n");
    let rig = Rig::with("team_phase", &settings, hang).await;
    let phased = |text: &str, phase: &str| NewTask {
        phase: phase.into(),
        ..task(text)
    };
    let ids = rig
        .plan(&[phased("older", "b"), phased("one", "a"), phased("two", "a")])
        .await;

    assert_eq!(rig.run().await, 2);
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Open);
    assert_ne!(rig.status(&ids[1]).await, PlanStatus::Open);
    assert_ne!(rig.status(&ids[2]).await, PlanStatus::Open);

    let before = rig.state().team_run(&rig.lead).await.unwrap();
    assert_eq!(rig.run().await, 0);
    assert_eq!(rig.state().team_run(&rig.lead).await.unwrap(), before);
    assert_eq!(rig.members().await.len(), 2);
    // A run may lower a part of the budget, and a later one does not raise it again.
    let fewer = |budget: TeamBudget| {
        TeamBudget::new(budget.at_once(), 20, budget.tokens(), budget.minutes()).unwrap()
    };
    rig.run_with(fewer).await.unwrap();
    rig.run().await;
    let run = rig.state().team_run(&rig.lead).await.unwrap().unwrap();
    assert_eq!(run.budget.steps(), 20);
}

#[tokio::test]
async fn tasks_with_overlapping_files_never_work_together() {
    let rig = Rig::new("team_files", hang).await;
    let files = |text: &str, entry: &str| NewTask {
        files: TaskFiles::new([entry]).unwrap(),
        ..task(text)
    };
    let ids = rig
        .plan(&[
            files("the folder", "src/"),
            files("a file in it", "src/x.rs"),
        ])
        .await;

    assert_eq!(rig.run().await, 1);
    for _ in 0..3 {
        assert_eq!(rig.pass().await, 0);
    }
    assert_eq!(rig.status(&ids[1]).await, PlanStatus::Open);
    // The claim that lost left one thread, which every later pass took again.
    assert_eq!(rig.members().await.len(), 2);
    let spare = rig.state().spare_member(&rig.lead, "general").await;
    assert!(spare.unwrap().is_some());
}

#[tokio::test]
async fn a_task_lost_three_times_fails_and_the_lead_gets_one_line() {
    let rig = Rig::new("team_fails", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Reject,
    })
    .await;
    let ids = rig
        .plan(&[
            NewTask {
                key: Some("a".into()),
                ..task("cannot be done")
            },
            NewTask {
                after: vec!["a".into()],
                ..task("behind it")
            },
        ])
        .await;
    rig.run().await;
    until("the task failed", || async {
        rig.status(&ids[0]).await == PlanStatus::Failed
    })
    .await;

    let failed = rig.task(&ids[0]).await;
    assert_eq!(failed.attempts, 3);
    let behind = rig.task(&ids[1]).await;
    assert_eq!(behind.status, PlanStatus::Blocked);
    assert_eq!(behind.waits_on, Some(WaitsOn::Task(ids[0].clone())));
    let members = rig.members().await;
    assert_eq!(members.len(), 3, "a new thread for each attempt");
    for (attempt, member) in members.iter().enumerate() {
        let card = format!("card:{}:{}", ids[0], attempt + 1);
        assert_eq!(rig.uids(member).await, [card]);
    }
    until("the lead read the line", || async {
        rig.turns(&rig.lead).await == (1, false)
    })
    .await;
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("failed: its turn failed: bad request after 3 attempts."));
    // The lead's turn changed nothing, and nothing can start: the stall rule ends it.
    rig.pass().await;
    assert_eq!(rig.ended().await, Some(TeamEnd::Stalled));
}

#[tokio::test]
async fn a_member_that_says_blocked_blocks_its_task_with_its_note() {
    let blocked = "Looked at it.\nStatus: blocked\nResult: nothing yet\nChecked: not run\n\
                   Open: needs the staging URL [and] \u{202e}more";
    let rig = Rig::new("team_blocked", move |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false => Out::Say(blocked.into()),
    })
    .await;
    let ids = rig.plan(&[task("deploy it")]).await;
    rig.run().await;
    until("the task is blocked", || async {
        rig.status(&ids[0]).await == PlanStatus::Blocked
    })
    .await;

    let task = rig.task(&ids[0]).await;
    assert_eq!(task.note, "needs the staging URL (and)  more");
    until("the lead read the line", || async {
        rig.turns(&rig.lead).await == (1, false)
    })
    .await;
    let lines = rig.lines().await;
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with(
        "blocked: needs the staging URL (and)  more. Answer by reopening it and editing its \
         brief, or fail it."
    ));
}

#[tokio::test]
async fn a_lease_that_runs_out_gives_the_task_to_a_new_member() {
    let rig = Rig::new("team_lease", hang).await;
    let ids = rig.plan(&[task("slow work")]).await;
    rig.run().await;
    until("the member's turn runs", || async {
        rig.status(&ids[0]).await == PlanStatus::Running
    })
    .await;
    let first = rig.members().await.remove(0);
    // Within the lease nothing moves.
    let now = timestamp_ms().unwrap();
    let launcher = rig.launcher();
    launcher
        .team_pass_at(&rig.lead, now + LEASE_MS - 30_000)
        .await
        .unwrap();
    assert_eq!(rig.task(&ids[0]).await.owner_thread_id, Some(first.clone()));

    let later = now + LEASE_MS + 30_000;
    assert_eq!(launcher.team_pass_at(&rig.lead, later).await.unwrap(), 1);

    let task = rig.task(&ids[0]).await;
    assert_eq!(task.attempts, 2);
    let members = rig.members().await;
    assert_eq!(members.len(), 2);
    assert_eq!(task.owner_thread_id.as_ref(), Some(&members[1]));
    assert_eq!(rig.uids(&members[1]).await, [format!("card:{}:2", ids[0])]);
    // The member that lost the task was stopped, and its stop is not the user's.
    until("the first member stopped", || async {
        !rig.turns(&first).await.1
    })
    .await;
    assert_eq!(rig.ended().await, None);
    // Its heartbeat renews nothing it does not own.
    let renewed = rig.state().renew_lease(&first, &ids[0], later).await;
    assert!(renewed.is_err());
}

#[tokio::test]
async fn a_card_delivered_again_is_dropped() {
    let rig = Rig::new("team_card_twice", hang).await;
    let ids = rig
        .plan(&[NewTask {
            acceptance: "the test passes".into(),
            gate: Some(GoalCheck::UserConfirmation),
            ..task("write [it]")
        }])
        .await;
    rig.run().await;
    let member = rig.members().await.remove(0);
    let task = rig.task(&ids[0]).await;
    for _ in 0..2 {
        super::super::member::deliver_card(rig.launcher(), &rig.lead, &member, &task, &rig.folder)
            .await
            .unwrap();
        rig.pass().await;
    }
    assert_eq!(rig.uids(&member).await, [format!("card:{}:1", ids[0])]);
    until("the member read its card", || async {
        !rig.state().has_pending(&member).await.unwrap()
    })
    .await;
    let messages = rig.state().list_messages(&member).await.unwrap();
    let cards: Vec<_> = messages
        .iter()
        .filter(|message| message.from_thread_id.is_some())
        .collect();
    assert_eq!(cards.len(), 1);
    let id = crate::state::short_task_id(&ids[0]);
    let head = format!(
        "Task #{id}: write (it)\nAccept: the test passes\nGate: the user confirms it\n\
         Yours: no file; change nothing\nRules: "
    );
    assert!(cards[0].content.starts_with(&head), "{}", cards[0].content);
}

#[tokio::test]
async fn a_task_the_lead_sends_back_wakes_the_same_member_once() {
    let rig = Rig::new("team_rework", all_done).await;
    let ids = rig.plan(&[task("write it")]).await;
    rig.run().await;
    until("the member handed in", || async {
        rig.status(&ids[0]).await == PlanStatus::Review
    })
    .await;
    let member = rig.members().await.remove(0);

    // The lead's review rejects it; its tool then says the plan changed.
    let now = timestamp_ms().unwrap();
    let state = rig.state();
    state
        .rework_task(&rig.lead, &ids[0], "use tabs", now)
        .await
        .unwrap();
    rig.launcher().team_changed(&rig.lead).await;
    rig.pass().await;

    until("the member handed in again", || async {
        rig.status(&ids[0]).await == PlanStatus::Review && rig.turns(&member).await == (2, false)
    })
    .await;
    let task = rig.task(&ids[0]).await;
    assert_eq!((task.reworks, task.attempts), (1, 1));
    assert_eq!(task.owner_thread_id, Some(member.clone()));
    assert_eq!(rig.members().await.len(), 1);
    let uids = rig.uids(&member).await;
    assert_eq!(
        uids,
        [format!("card:{}:1", ids[0]), format!("rework:{}:1", ids[0])]
    );
    let waits = |line: &String| line.contains("waits for you");
    assert_eq!(
        rig.lines().await.iter().filter(|line| waits(line)).count(),
        2
    );
}
