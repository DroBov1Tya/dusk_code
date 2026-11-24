//! A task its member lost goes back to that member, with its transcript and a short
//! resume note, once; a loss that would only repeat goes to a fresh member, whose card
//! says how the last attempt ended. Each member says what it does.

use super::*;
use crate::providers::ProviderErrorKind;

/// A member that fails on its card with `fails`, and finishes when given the task back
/// or a card that says how the last attempt ended.
fn fails_once(fails: fn() -> Out) -> impl Fn(&Asked) -> Out {
    move |asked| match asked.lead {
        true => Out::Say("Noted.".into()),
        false if asked.user.contains("Your last turn on") => Out::Say(DONE.into()),
        false if asked.user.contains("Before you") => Out::Say(DONE.into()),
        false => fails(),
    }
}

#[tokio::test]
async fn a_lost_task_goes_back_to_its_member_with_a_resume_note() {
    let rig = Rig::new("team_resume", fails_once(|| Out::Reject)).await;
    let id = rig
        .plan(&[NewTask {
            role: Some("general".into()),
            ..task("parse the file and keep its headers")
        }])
        .await
        .remove(0);
    rig.run().await;
    until("the task waits for the lead", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;

    let members = rig.members().await;
    assert_eq!(members.len(), 1, "no second member");
    let member = &members[0];
    let task = rig.task(&id).await;
    assert_eq!((task.attempts, task.resumes), (2, 1));
    assert_eq!(task.owner_thread_id.as_ref(), Some(member));
    assert_eq!(
        rig.uids(member).await,
        [format!("card:{id}:1:{member}"), format!("resume:{id}:1")]
    );
    let messages = rig.state().list_messages(member).await.unwrap();
    let note = messages
        .iter()
        .find(|message| message.content.starts_with("Your last turn on"))
        .unwrap();
    assert!(
        note.content
            .contains("stopped: its turn failed: bad request. Continue from where you are"),
        "{}",
        note.content
    );
    // Who does what, as clients and the lead's lines show it.
    let team = rig.state().team_members(&rig.lead).await.unwrap();
    assert_eq!(team[0].summary, "parse the file and keep its headers");
    let name = rig
        .state()
        .load_thread(member)
        .await
        .unwrap()
        .unwrap()
        .name
        .unwrap();
    let who = format!("({name}, general: parse the file and keep its headers) waits for you");
    until("the lead was told who did it", || async {
        rig.lines().await.iter().any(|line| line.contains(&who))
    })
    .await;
}

#[tokio::test]
async fn a_request_the_provider_refused_goes_to_a_fresh_member() {
    let rig = Rig::new(
        "team_resume_refused",
        fails_once(|| Out::Fail(ProviderErrorKind::Refused)),
    )
    .await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    rig.run().await;
    until("the task waits for the lead", || async {
        rig.status(&id).await == PlanStatus::Review
    })
    .await;

    // It would be refused again: a fresh thread, whose card says how the last ended.
    let members = rig.members().await;
    assert_eq!(members.len(), 2);
    let task = rig.task(&id).await;
    assert_eq!((task.attempts, task.resumes), (2, 0));
    assert_eq!(task.owner_thread_id.as_ref(), Some(&members[1]));
    let messages = rig.state().list_messages(&members[1]).await.unwrap();
    let card = &messages
        .iter()
        .find(|message| message.content.starts_with("Task #"))
        .unwrap()
        .content;
    assert!(
        card.contains("\nBefore you (how the last attempt ended): its turn failed: "),
        "{card}"
    );
}

#[tokio::test]
async fn an_edited_task_is_not_given_back() {
    let rig = Rig::new("team_resume_edited", fails_once(|| Out::Reject)).await;
    let id = rig.plan(&[task("one")]).await.remove(0);
    let now = crate::state::timestamp_ms().unwrap();
    rig.state().start_team(&rig.lead, 1, now).await.unwrap();
    // Lost once by a member, then edited by the lead before it is given out again.
    let paths = &rig.launcher().paths;
    let lost = crate::runtime::thread::start::start_member_thread(
        rig.state(),
        paths,
        &rig.lead,
        "general",
        None,
    )
    .await
    .unwrap()
    .thread_id;
    let claim = crate::state::Claim {
        owner: &lost,
        at_once: 1,
        model: None,
        now,
    };
    rig.state().claim_task(&rig.lead, &id, claim).await.unwrap();
    let released = rig
        .state()
        .release_task(&rig.lead, &id, "its turn failed")
        .await
        .unwrap();
    assert_eq!(
        released.last_owner_thread_id.as_deref(),
        Some(lost.as_str())
    );
    let edit = crate::state::TaskEdit {
        brief: Some("use the new parser".into()),
        ..Default::default()
    };
    let edited = rig
        .state()
        .edit_plan_task(&rig.lead, &id, &edit)
        .await
        .unwrap();
    assert_eq!(edited.last_owner_thread_id, None);

    rig.pass().await;
    // It gets a card: the thread, which never had a turn, takes it as a spare would.
    let task = rig.task(&id).await;
    assert_eq!(task.resumes, 0);
    let owner = task.owner_thread_id.unwrap();
    assert_eq!(rig.uids(&owner).await, [format!("card:{id}:2:{owner}")]);
}
