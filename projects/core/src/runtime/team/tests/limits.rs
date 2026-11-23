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

#[test]
fn a_task_answers_on_its_tier_below_the_roles_own_model() {
    use crate::config::Config;
    use crate::runtime::tiers::Tier;
    let config = Config::parse(
        b"providers:\n  big:\n    base_url: https://127.0.0.1:1/v1\n\
          models:\n  strong: big/fable\n  weak: gone/haiku\n\
          team:\n  models:\n    code-reviewer: big/opus\n",
    )
    .unwrap();
    let name = |role: &str, tier| super::super::member::model_name(&config, role, tier, None);
    assert_eq!(
        name("general", Some(Tier::Strong)).as_deref(),
        Some("big/fable")
    );
    // The role's own model is the user's word for it, above any tier.
    assert_eq!(
        name("code-reviewer", Some(Tier::Strong)).as_deref(),
        Some("big/opus")
    );
    // Unset, or set to a provider not connected: the chat's model, here none.
    assert_eq!(name("general", Some(Tier::Default)), None);
    assert_eq!(name("general", Some(Tier::Weak)), None);
    assert_eq!(name("general", None), None);
}

#[tokio::test]
async fn a_task_with_an_unset_tier_runs_on_the_chats_model_at_its_effort() {
    use crate::config::Effort;
    use crate::runtime::tiers::Tier;
    let rig = Rig::with("team_tier_effort", "mode: TEAM\neffort: high\n", hang).await;
    let ids = rig
        .plan(&[NewTask {
            tier: Some(Tier::Weak),
            effort: Some(Effort::Low),
            ..task("scout")
        }])
        .await;
    rig.run().await;
    let given = rig.task(&ids[0]).await;
    assert_eq!(given.tier, Some(Tier::Weak));
    assert_eq!(given.model.as_deref(), Some("fake/scripted"));
    let members = rig.members().await;
    let effort = rig.state().agent_effort(&members[0]).await.unwrap();
    assert_eq!(effort, Some(Effort::Low));
}

#[tokio::test]
async fn a_mode_switch_for_another_chat_does_not_pause_the_team() {
    let rig = Rig::new("team_mode_left", hang).await;
    // The user writes to the chat in TEAM mode: the chat takes that mode up.
    rig.input.send("lead the work").await.unwrap();
    until("the chat's turn ended", || async {
        rig.turns(&rig.lead).await == (1, false)
    })
    .await;
    assert_eq!(
        rig.state().chat_mode(&rig.lead).await.unwrap().as_deref(),
        Some("TEAM")
    );
    rig.plan(&[task("one"), task("two")]).await;
    let config = &rig.launcher().config;
    // The setting moves on, as when the user switches the mode in another chat.
    config
        .update(|config| config.set_mode("MAIN".into()))
        .unwrap();
    let one_at_once =
        |budget: TeamBudget| TeamBudget::new(1, budget.steps(), budget.tokens(), budget.minutes());
    assert_eq!(
        rig.run_with(|budget| one_at_once(budget).unwrap())
            .await
            .unwrap(),
        1
    );
    // The member at work is gone and its task is back: a slot is free.
    let free_a_slot = || async {
        for task in rig.state().plan(&rig.lead).await.unwrap() {
            let Some(member) = task.owner_thread_id.filter(|_| {
                !matches!(
                    task.status,
                    PlanStatus::Open | PlanStatus::Done | PlanStatus::Failed
                )
            }) else {
                continue;
            };
            rig.state()
                .release_task(&rig.lead, &task.task_id, "lost")
                .await
                .unwrap();
            super::super::end::cancel_member(rig.launcher(), &member).await;
        }
    };
    free_a_slot().await;
    assert_eq!(rig.pass().await, 1);

    // The user writes to this chat in MAIN mode: its team gives out nothing new.
    free_a_slot().await;
    rig.input.send("plain work now").await.unwrap();
    until("the chat's second turn ended", || async {
        rig.turns(&rig.lead).await == (2, false)
    })
    .await;
    assert_eq!(rig.pass().await, 0);
    assert!(rig.run_with(|budget| budget).await.is_err());
    assert_eq!(rig.ended().await, None);

    config
        .update(|config| config.set_mode("TEAM".into()))
        .unwrap();
    rig.input.send("lead again").await.unwrap();
    until("the chat's third turn ended", || async {
        rig.turns(&rig.lead).await == (3, false)
    })
    .await;
    assert_eq!(rig.pass().await, 1);
}
