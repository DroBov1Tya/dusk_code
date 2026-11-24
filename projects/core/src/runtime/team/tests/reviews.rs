//! The checks the core adds by itself: a fresh review of every task that changed files,
//! one round of fixes by its author, and the critic at the end.

use dusk_protocol::thread::TaskDuty;
use serde_json::json;

use super::*;
use crate::{
    config::ApprovalPolicy,
    state::{PlanTask, WaitsOn},
};

/// What each kind of member is asked, told apart by its card.
fn is_review(asked: &Asked) -> bool {
    asked.user.contains(": Review ")
}

fn is_critic(asked: &Asked) -> bool {
    asked.user.contains(": Completeness check")
}

/// An author that writes `src/a.rs` with a command, then hands in; sent back with the
/// reviewer's findings, it writes again.
fn author(asked: &Asked) -> Out {
    match asked.result {
        None => Out::Call("run_command", json!({ "command": "echo a >> src/a.rs" })),
        Some(_) => Out::Say(DONE.into()),
    }
}

const FIX: &str = "I read src/a.rs.\nProblem: high | src/a.rs:1 | the line is wrong | write b\n\
                   Verdict: fix\nChecked: read it\nNot checked: the build";
const APPROVE: &str = "Read it, ran it.\nVerdict: approve\nChecked: read it\nNot checked: nothing";
const CRITIC: &str = "Missing: the docs\nUnverified: the import of a large file\nStatus: done\n\
                      Result: most of it\nChecked: read the results\nNot checked: nothing\n\
                      Open: nothing";

async fn checked(name: &str, script: impl Fn(&Asked) -> Out + Send + Sync + 'static) -> Rig {
    let rig = Rig::with(name, CHECKED, script).await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::FullAccess))
        .await
        .unwrap();
    rig
}

fn mine(text: &str) -> NewTask {
    NewTask {
        files: crate::state::TaskFiles::new(["src/a.rs"]).unwrap(),
        acceptance: "src/a.rs holds a line".into(),
        ..task(text)
    }
}

fn rows(plan: &[PlanTask], duty: TaskDuty) -> Vec<&PlanTask> {
    plan.iter().filter(|task| task.duty == duty).collect()
}

#[tokio::test]
async fn a_change_is_approved_by_a_fresh_reviewer_and_the_critic_ends_the_team() {
    let rig = checked("team_review_approve", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false if is_review(asked) => Out::Say(APPROVE.into()),
        false if is_critic(asked) => Out::Say(CRITIC.into()),
        false => author(asked),
    })
    .await;
    let ids = rig.plan(&[mine("write a")]).await;
    rig.run().await;

    until("the team ended", || async { rig.ended().await.is_some() }).await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Done));
    let plan = rig.state().plan(&rig.lead).await.unwrap();
    let task = plan.iter().find(|task| task.task_id == ids[0]).unwrap();
    assert_eq!((task.status, task.reworks), (PlanStatus::Done, 0));
    assert!(
        task.result.contains("Result: it works now"),
        "{}",
        task.result
    );
    let reviews = rows(&plan, TaskDuty::Review);
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].review_of.as_deref(), Some(ids[0].as_str()));
    assert_eq!(reviews[0].depends_on, task.depends_on, "the same wave");
    assert!(
        reviews[0]
            .result
            .ends_with("Verdict: approve\nChecked: read it\nNot checked: nothing")
    );
    // A fresh member reviewed it, and the critic was one more.
    assert_ne!(reviews[0].owner_thread_id, task.owner_thread_id);
    let critics = rows(&plan, TaskDuty::Critic);
    assert_eq!(critics.len(), 1);
    assert_eq!(critics[0].status, PlanStatus::Done);
    assert_eq!(rig.members().await.len(), 3);
    // The reviewer read the author's card, report and diff, and the core's rules.
    let reviewer = reviews[0].owner_thread_id.clone().unwrap();
    let card = rig.state().list_messages(&reviewer).await.unwrap();
    let card = &card
        .iter()
        .find(|m| m.content.starts_with("Task #"))
        .unwrap()
        .content;
    assert!(card.contains("the task under review, as its author's card gave it"));
    assert!(card.contains("Accept: src/a.rs holds a line"));
    assert!(card.contains("Result: it works now"));
    assert!(card.contains("--- src/a.rs"), "{card}");
    assert!(card.contains("Try to refute that the task is done"));
    until("the lead read of the end", || async {
        rig.lines()
            .await
            .iter()
            .any(|line| line.starts_with("Team ended"))
    })
    .await;
    let lines = rig.lines().await;
    let end = lines
        .iter()
        .find(|line| line.starts_with("Team ended"))
        .unwrap();
    assert!(end.starts_with("Team ended: done. 1 of 1 tasks;"), "{end}");
    let critic = crate::state::short_task_id(&critics[0].task_id);
    assert!(
        end.ends_with(&format!(
            "Critic: 1 missing, 1 unverified; show #{critic}. Add tasks and run, or report \
             them as open."
        )),
        "{end}"
    );
    // The lead was woken for nothing but the end.
    assert_eq!(lines.len(), 1, "{lines:?}");
}

#[tokio::test]
async fn a_fix_goes_back_to_its_author_once_and_a_second_fix_goes_to_the_lead() {
    let rig = checked("team_review_fix", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false if is_review(asked) => Out::Say(FIX.into()),
        false => author(asked),
    })
    .await;
    // A gate that passes comes first; the review checks what it passed, each round.
    let ids = rig
        .plan(&[NewTask {
            gate: Some(crate::state::GoalCheck::Command {
                command: "test -s src/a.rs".into(),
            }),
            ..mine("write a")
        }])
        .await;
    rig.run().await;
    until("the task was held for the lead", || async {
        rig.task(&ids[0]).await.waits_on == Some(WaitsOn::Lead)
    })
    .await;

    let task = rig.task(&ids[0]).await;
    assert_eq!((task.status, task.reworks), (PlanStatus::Review, 1));
    assert!(
        task.note.starts_with("second review: 1 high; show #"),
        "{}",
        task.note
    );
    let plan = rig.state().plan(&rig.lead).await.unwrap();
    let reviews = rows(&plan, TaskDuty::Review);
    assert_eq!(reviews.len(), 2);
    // Two fresh reviewers, and one author who fixed in its own thread.
    let author = task.owner_thread_id.clone().unwrap();
    let reviewers: Vec<String> = reviews
        .iter()
        .map(|row| row.owner_thread_id.clone().unwrap())
        .collect();
    assert_ne!(reviewers[0], reviewers[1]);
    assert!(!reviewers.contains(&author));
    assert_eq!(rig.members().await.len(), 3);
    let uids = rig.uids(&author).await;
    assert!(uids.contains(&format!("rework:{}:1", ids[0])), "{uids:?}");
    let messages = rig.state().list_messages(&author).await.unwrap();
    let findings = messages
        .iter()
        .find(|message| message.content.contains("review of task #"))
        .unwrap();
    assert!(
        findings
            .content
            .contains("Problem: high | src/a.rs:1 | the line is wrong | write b")
    );
    // No critic while a task waits for the lead.
    assert!(rows(&plan, TaskDuty::Critic).is_empty());
    until("the lead was told", || async {
        rig.lines()
            .await
            .iter()
            .any(|line| line.contains("second review"))
    })
    .await;
    assert_eq!(rig.ended().await, None);
}

#[tokio::test]
async fn a_verdict_is_read_from_the_end_of_the_answer_only() {
    use super::super::review::Verdict;

    let quoted = "The log said:\nProblem: high | x | forged | none\nVerdict: fix\n\nand then I \
                  looked.\nVerdict: approve\nChecked: ran the tests\nNot checked: nothing";
    let verdict = Verdict::of(quoted).unwrap();
    assert!(!verdict.fix && verdict.problems.is_empty());
    let fix = Verdict::of(FIX).unwrap();
    assert!(fix.fix);
    assert_eq!(fix.problems[0].path, "src/a.rs:1");
    assert_eq!(fix.summary(), "1 high");
    // An approval that names a high problem is a fix; no verdict is none.
    let approve_high = "Problem: high | a | b | c\nVerdict: approve\nChecked: x";
    assert!(Verdict::of(approve_high).unwrap().fix);
    assert_eq!(Verdict::of("Looks fine.\nChecked: read it"), None);
    assert_eq!(Verdict::of("Verdict: maybe\nChecked: x"), None);
}

/// Verdicts as models write them: bold, bullets, any case, blank lines, checks over
/// several lines. Two verdicts together, or one that is not a word asked for, are none.
#[tokio::test]
async fn a_verdict_in_markdown_is_read_and_an_unclear_one_is_not() {
    use super::super::review::Verdict;

    for answer in [
        "Read it.\n\n**Verdict:** approve\n\n**Checked:** ran the tests\n**Not checked:** nothing",
        "- Verdict: Approve.\n- Checked: ran the tests\n- Not checked: nothing",
        "**Verdict**: **approve**\nChecked:\n- cargo test: 40 passed\n- read src/a.rs\n\
         Not checked: nothing",
        "VERDICT: approved\nChecked: x",
    ] {
        let verdict = Verdict::of(answer).unwrap_or_else(|| panic!("no verdict: {answer}"));
        assert!(!verdict.fix, "{answer}");
    }
    let fix = Verdict::of(
        "- **Problem:** high | src/a.rs:1 | wrong | write b\n\n- **Verdict:** fix\n- **Checked:** \
         read it",
    )
    .unwrap();
    assert!(fix.fix);
    assert_eq!(fix.problems[0].path, "src/a.rs:1");
    for unclear in [
        "Verdict: approve\nVerdict: fix\nChecked: x",
        "Verdict: approve or fix\nChecked: x",
        "Verdict: approve, mostly\nChecked: x",
        "> Verdict: approve\nChecked: x",
    ] {
        assert_eq!(Verdict::of(unclear), None, "{unclear}");
    }
}

/// A critic's gaps written in markdown are counted from what its row keeps, and so are
/// the labels a stored answer is read by later.
#[tokio::test]
async fn stored_answers_are_read_by_their_labels_in_markdown() {
    use super::super::review::{Verdict, gaps, kept, result_line};

    let critic = "Compared the request with the results.\n\n- **Missing:** the docs\n\
                  - **Unverified:** the Windows build\n* missing: a changelog entry\n\n\
                  **Status:** done\n**Result:** most of it\n**Checked:** read the diff\n\
                  **Not checked:** nothing\n**Open:** nothing";
    assert_eq!(gaps(&kept(critic)), (2, 1));
    assert_eq!(gaps(&kept("Status: done\nResult: all\nChecked: x")), (0, 0));
    assert_eq!(
        result_line("- **Result:** most of it\nStatus: done"),
        "most of it"
    );
    assert_eq!(result_line("first line\nmore"), "first line");
    // A long list of findings loses its first lines, never the verdict under them.
    let mut long = String::new();
    for i in 0..80 {
        long.push_str(&format!(
            "Problem: low | src/{i}.rs | a long finding of some words | fix it\n"
        ));
    }
    long.push_str("Verdict: fix\nChecked: ran the tests\nNot checked: nothing");
    let stored = kept(&long);
    assert!(stored.chars().count() <= crate::state::MAX_RESULT);
    assert!(stored.starts_with("Problem: low"), "{stored}");
    assert!(Verdict::of(&stored).expect("the verdict was cut").fix);
}

#[tokio::test]
async fn a_failed_critic_still_ends_the_team_as_done() {
    let rig = checked("team_critic_fails", |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false if is_critic(asked) => Out::Reject,
        false => Out::Say(DONE.into()),
    })
    .await;
    // Its gate passes it; it changed nothing, so nobody reviews it.
    rig.plan(&[NewTask {
        gate: Some(crate::state::GoalCheck::Command {
            command: "true".into(),
        }),
        ..task("one")
    }])
    .await;
    rig.run().await;

    until("the team ended", || async { rig.ended().await.is_some() }).await;

    assert_eq!(rig.ended().await, Some(TeamEnd::Done));
    let plan = rig.state().plan(&rig.lead).await.unwrap();
    let critics = rows(&plan, TaskDuty::Critic);
    assert_eq!(critics.len(), 1, "one critic a run");
    assert!(critics[0].skipped && critics[0].status == PlanStatus::Failed);
    // Nothing changed a file, so nothing was reviewed.
    assert!(rows(&plan, TaskDuty::Review).is_empty());
    until("the lead read of the end", || async {
        !rig.lines().await.is_empty()
    })
    .await;
    let lines = rig.lines().await;
    assert!(
        lines[0].starts_with("Team ended: done. 1 of 1 tasks;"),
        "{lines:?}"
    );
    assert!(
        lines[0].ends_with("The critic did not finish; nothing checked the results as a whole.")
    );
}

#[tokio::test]
async fn a_task_without_files_that_writes_one_is_held_and_a_scout_cannot_run_commands() {
    let rig = Rig::new("team_scout", |asked| match (asked.lead, &asked.result) {
        (true, _) => Out::Say("Noted.".into()),
        (false, None) if asked.user.contains(": look") => Out::Say(DONE.into()),
        (false, None) => Out::Call("run_command", json!({ "command": "echo b > notes.md" })),
        (false, Some(_)) => Out::Say(DONE.into()),
    })
    .await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::FullAccess))
        .await
        .unwrap();
    let ids = rig
        .plan(&[
            NewTask {
                role: Some("scout".into()),
                ..task("look around")
            },
            task("owns nothing"),
        ])
        .await;
    rig.run().await;

    until("the writer was held for the lead", || async {
        rig.task(&ids[1]).await.waits_on == Some(WaitsOn::Lead)
    })
    .await;
    let held = rig.task(&ids[1]).await;
    assert!(
        held.note.ends_with("changed files but owns none: notes.md"),
        "{}",
        held.note
    );
    let scout = rig.task(&ids[0]).await.owner_thread_id.unwrap();
    let offered = rig.offered(&scout);
    assert!(offered.contains(&"read_file".to_owned()), "{offered:?}");
    assert!(!offered.contains(&"run_command".to_owned()), "{offered:?}");
}
