//! What a member may do is the core's to fix, and with the switch off nothing runs.

use super::*;
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
async fn a_member_has_no_tool_to_start_agents_change_the_plan_or_write_to_the_lead() {
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
        for denied in DENIED {
            assert!(
                !offered.contains(&denied.to_owned()),
                "{denied} in {offered:?}"
            );
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
    let mut agent = rig.launcher().tools("t", None, 1);
    assert!(agent.names().any(|name| name == "message_parent"));
    super::super::limit_tools(&mut agent);
    assert!(!agent.names().any(|name| DENIED.contains(&name)));
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
