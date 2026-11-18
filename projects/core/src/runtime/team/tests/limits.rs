//! What a member may do is the core's to fix, and with the switch off nothing runs.

use super::{super::member::MEMBER_TOOLS, *};
use crate::config::ApprovalPolicy;

const DENIED: [&str; 6] = [
    "ask_agent",
    "tasks",
    "remember",
    "remember_project",
    "message_parent",
    "agent_state",
];

#[tokio::test]
async fn a_member_has_only_the_tools_of_the_allowlist() {
    let rig = Rig::new("team_tools", |asked| match asked.lead {
        true => Out::Say("Hello.".into()),
        false => Out::Hang,
    })
    .await;
    rig.state()
        .set_thread_approvals(&rig.lead, Some(ApprovalPolicy::Sandbox))
        .await
        .unwrap();
    rig.plan(&[task("one"), task("two")]).await;
    rig.run().await;
    let members = rig.members().await;
    until("both members were asked", || async {
        members.iter().all(|member| !rig.offered(member).is_empty())
    })
    .await;

    for member in &members {
        let offered = rig.offered(member);
        assert!(offered.contains(&"run_command".to_owned()), "{offered:?}");
        for tool in &offered {
            assert!(MEMBER_TOOLS.contains(&tool.as_str()), "{tool} is offered");
        }
        // The lead's mode, copied: no wider than what the lead may do unasked.
        let policy = rig.state().thread_approvals(member).await.unwrap();
        assert_eq!(policy, Some(ApprovalPolicy::Sandbox));
        assert_eq!(rig.state().thread_depth(member).await.unwrap(), 1);
    }
    // The lead itself keeps its plan and its agents.
    rig.input.send("hello").await.unwrap();
    until("the lead was asked", || async {
        !rig.offered(&rig.lead).is_empty()
    })
    .await;
    let lead = rig.offered(&rig.lead);
    assert!(lead.contains(&"tasks".to_owned()) && lead.contains(&"ask_agent".to_owned()));
    // An agent the lead asks is no member and keeps an agent's tools.
    let config = rig.launcher().config.current();
    let mut agent = rig.launcher().tools("t", None, 1);
    assert!(agent.names().any(|name| name == "message_parent"));
    super::super::limit_tools(&mut agent, &config, "general");
    assert!(!agent.names().any(|name| DENIED.contains(&name)));
}

#[tokio::test]
async fn a_member_that_calls_a_tool_it_was_not_given_is_refused() {
    let calls = AtomicUsize::new(0);
    // The member tries to rewrite the plan and to start an agent before it answers.
    let rig = Rig::with(
        "team_tool_refused",
        "mode: TEAM\nteam:\n  tools:\n    general: [read_file, tasks, ask_agent]\n",
        move |asked| match asked.lead {
            true => Out::Say("Noted.".into()),
            false => match calls.fetch_add(1, Ordering::Relaxed) {
                0 => Out::Call(
                    "tasks",
                    serde_json::json!({ "action": "add", "tasks": [{ "text": "mine" }] }),
                ),
                1 => Out::Call(
                    "ask_agent",
                    serde_json::json!({ "agent": "general", "task": "do it" }),
                ),
                _ => Out::Say(DONE.into()),
            },
        },
    )
    .await;
    let ids = rig.plan(&[task("one")]).await;
    rig.run().await;

    until("the member handed in", || async {
        rig.status(&ids[0]).await == PlanStatus::Review
    })
    .await;

    // The settings named `tasks` and `ask_agent` for the role, and gave neither: a
    // list narrows what a member may have and adds nothing to it.
    let member = rig.members().await.remove(0);
    assert_eq!(rig.offered(&member), ["read_file"]);
    let results: Vec<String> = sqlx::query_scalar(
        "SELECT content FROM messages WHERE thread_id = ? AND content LIKE '%unknown tool%'",
    )
    .bind(&member)
    .fetch_all(&rig.state().pool)
    .await
    .unwrap();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].contains("unknown tool tasks"));
    assert!(results[1].contains("unknown tool ask_agent"));
    assert_eq!(rig.state().plan(&rig.lead).await.unwrap().len(), 1);
    assert_eq!(rig.members().await.len(), 1);
}

#[tokio::test]
async fn with_the_switch_off_nothing_runs_and_the_lead_has_the_board() {
    let rig = Rig::with(
        "team_off",
        "mode: TEAM\nlimits:\n  plan_graph: false\n",
        all_done,
    )
    .await;
    let ids = rig.plan(&[task("one")]).await;

    let refused = rig.run_with(|budget| budget).await;

    assert!(refused.unwrap_err().to_string().contains("switched off"));
    assert_eq!(rig.pass().await, 0);
    assert_eq!(rig.state().team_run(&rig.lead).await.unwrap(), None);
    assert_eq!(rig.status(&ids[0]).await, PlanStatus::Open);
    assert!(rig.members().await.is_empty());
    // The chat's `tasks` is byte for byte what an agent's is: the board.
    let launcher = rig.launcher();
    let scope = Arc::new(launcher.scope(&rig.folder, &rig.folder).unwrap());
    let config = launcher.config.current();
    let spec = |depth| {
        let specs = launcher.tools("t", None, depth).specs(&scope, &config);
        let tasks = specs.into_iter().find(|spec| spec.name == "tasks").unwrap();
        (tasks.description, tasks.parameters.to_string())
    };
    assert_eq!(spec(0), spec(1));
    assert!(!spec(0).0.contains("the core runs it"));
}

#[tokio::test]
async fn a_role_answers_on_its_own_model_and_one_that_cannot_answer_fails_the_task() {
    // `fake` is the chat's provider in a test, and no provider of the config: a role
    // set to it could never answer. A role with no setting follows the chat.
    let rig = Rig::with(
        "team_role_model",
        "mode: TEAM\nteam:\n  models:\n    code-reviewer: nowhere/strong\n",
        hang,
    )
    .await;
    let ids = rig
        .plan(&[
            task("write"),
            NewTask {
                role: Some("code-reviewer".into()),
                ..task("review")
            },
        ])
        .await;
    rig.run().await;

    let written = rig.task(&ids[0]).await;
    assert_eq!(written.model.as_deref(), Some("fake/scripted"));
    assert_eq!(written.status, PlanStatus::Claimed);
    // Not moved to the chat's model behind the user's back: the lead reads why.
    let reviewed = rig.task(&ids[1]).await;
    assert_eq!(reviewed.status, PlanStatus::Failed);
    assert!(
        reviewed
            .note
            .contains("nowhere/strong set for code-reviewer names no provider"),
        "{}",
        reviewed.note
    );
    assert_eq!(rig.members().await.len(), 1);
}

#[tokio::test]
async fn outside_team_mode_nothing_is_given_out_and_the_mode_coming_back_goes_on() {
    let rig = Rig::new("team_mode_left", hang).await;
    let ids = rig.plan(&[task("one"), task("two")]).await;
    rig.run_with(|budget| {
        TeamBudget::new(1, budget.steps(), budget.tokens(), budget.minutes()).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(rig.status(&ids[1]).await, PlanStatus::Open);
    let config = &rig.launcher().config;
    config
        .update(|config| config.set_mode("MAIN".into()))
        .unwrap();
    // The member at work is gone and its task is back: a slot is free.
    let member = rig.members().await.remove(0);
    rig.state()
        .release_task(&rig.lead, &ids[0], "lost")
        .await
        .unwrap();
    super::super::end::cancel_member(rig.launcher(), &member).await;

    assert_eq!(rig.pass().await, 0);
    assert!(rig.run_with(|budget| budget).await.is_err());
    assert_eq!(rig.ended().await, None);

    config
        .update(|config| config.set_mode("TEAM".into()))
        .unwrap();
    assert_eq!(rig.pass().await, 1);
}
