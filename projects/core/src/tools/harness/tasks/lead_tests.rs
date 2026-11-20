//! The plan form of `tasks`: when it is offered, its texts, and each of the lead's
//! actions on untrusted arguments.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use dusk_protocol::thread::{GoalCheck, Team, TeamBudget};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::*;
use crate::{
    catalog::agents::Agents,
    state::{Claim, PassedBy, PlanStatus},
    testing::{temp_dir, tool_context_with},
    tools::ToolScope,
};

const LEAD: &str = "lead";
const ON: &str = "mode: TEAM\n";

struct Fixture {
    state: Arc<State>,
    tasks: Tasks,
    context: ToolContext,
    root: PathBuf,
}

async fn fixture(name: &str) -> Fixture {
    fixture_with(name, ON).await
}

/// A chat `lead` in a project with `src/main.rs`, members `m0` to `m3` (named `Ember-0`
/// and so on), an agent `reviewer` on disk that the catalog has not read yet.
async fn fixture_with(name: &str, settings: &str) -> Fixture {
    let root = temp_dir(name);
    let project = root.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(root.join("work")).unwrap();
    write_agent(&root, "reviewer");
    let state = Arc::new(State::in_memory().await.unwrap());
    state.create_thread(LEAD, "test").await.unwrap();
    for member in 0..4 {
        let id = format!("m{member}");
        let name = format!("Ember-{member}");
        state
            .create_agent_thread(&id, LEAD, "general", &name)
            .await
            .unwrap();
    }
    let agents = Agents::new(root.join("agents"), Arc::clone(&state));
    let tasks = Tasks::new(Arc::clone(&state), LEAD.into()).lead(agents);
    let scope = ToolScope::new(&project, &root.join("work")).unwrap();
    let context = ToolContext {
        state: Some(Arc::clone(&state)),
        thread_id: LEAD.into(),
        ..tool_context_with(&scope, settings)
    };
    Fixture {
        state,
        tasks,
        context,
        root,
    }
}

fn write_agent(root: &Path, name: &str) {
    let folder = root.join("agents").join(name);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("AGENT.md"),
        "---\ndescription: Reviews a change.\n---\nReview it.",
    )
    .unwrap();
}

impl Fixture {
    async fn call(&self, arguments: Value) -> anyhow::Result<String> {
        self.tasks.call(arguments, self.context.clone()).await
    }

    async fn refused(&self, arguments: Value) -> String {
        format!("{:#}", self.call(arguments).await.unwrap_err())
    }

    async fn ids(&self) -> Vec<String> {
        let plan = self.state.plan(LEAD).await.unwrap();
        plan.into_iter().map(|task| task.task_id).collect()
    }

    /// Moves an open task to `to` with `owner` as its member.
    async fn drive(&self, task: &str, owner: &str, to: PlanStatus) {
        let claim = Claim {
            owner,
            at_once: 8,
            model: None,
            now: timestamp(),
        };
        self.state.claim_task(LEAD, task, claim).await.unwrap();
        let turn_id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at)
             VALUES (?, ?, 1, 'running', 0)",
        )
        .bind(&turn_id)
        .bind(owner)
        .execute(&self.state.pool)
        .await
        .unwrap();
        self.state.start_task(owner, task, &turn_id).await.unwrap();
        if matches!(to, PlanStatus::Review | PlanStatus::Done) {
            self.state.hand_in(owner, task).await.unwrap();
        }
        if to == PlanStatus::Done {
            self.state
                .pass_task(LEAD, task, PassedBy::Lead)
                .await
                .unwrap();
        }
    }
}

fn timestamp() -> i64 {
    crate::state::timestamp_ms().unwrap()
}

fn spec_bytes(tasks: &Tasks, context: &ToolContext) -> (usize, usize, usize) {
    let schema = serde_json::to_string(&tasks.offered_parameters(context)).unwrap();
    (
        tasks.name().len(),
        tasks.description(context).len(),
        schema.len(),
    )
}

#[tokio::test]
async fn the_plan_form_is_offered_only_to_a_chat_in_team_mode() {
    let on = fixture("plan_offered").await;
    let board = Tasks::new(Arc::clone(&on.state), LEAD.into());
    let board_context = |settings: &str| ToolContext {
        config: tool_context_with(&on.context.scope, settings).config,
        ..on.context.clone()
    };
    let board_form = (
        board.description(&on.context),
        board.offered_parameters(&on.context),
    );
    for settings in [
        "limits:\n  plan_graph: true\n",
        "mode: PLAN\n",
        "mode: ORCHESTRATOR\n",
        "mode: TEAM\nlimits:\n  plan_graph: false\n",
    ] {
        let context = board_context(settings);
        assert_eq!(on.tasks.description(&context), board_form.0, "{settings}");
        assert_eq!(on.tasks.offered_parameters(&context), board_form.1);
    }
    // An agent's tasks (no lead) stay the board under a TEAM chat.
    assert_eq!(board.description(&on.context), board_form.0);
    assert_eq!(on.tasks.description(&on.context), text::DESCRIPTION);
    assert_eq!(on.tasks.offered_parameters(&on.context), text::schema());
}

#[tokio::test]
async fn the_plan_form_costs_what_the_design_says() {
    let f = fixture("plan_bytes").await;
    let plan = spec_bytes(&f.tasks, &f.context);
    assert_eq!(plan, (5, 1126, 969));
    let main = ToolContext {
        config: tool_context_with(&f.context.scope, "limits:\n  plan_graph: true\n").config,
        ..f.context.clone()
    };
    let board = spec_bytes(&f.tasks, &main);
    let growth = (plan.0 + plan.1 + plan.2) - (board.0 + board.1 + board.2);
    println!("board {board:?}, plan {plan:?}, growth {growth}");
    assert_eq!(board.0 + board.1 + board.2, 1274);
    assert_eq!(growth, 826);
}

fn seven() -> Value {
    json!({ "action": "add", "tasks": [
        { "key": "row", "text": "add the importer row type", "files": ["src/row.rs"] },
        { "key": "dto", "text": "make email optional in UserDto", "after": ["row"],
          "gate": "cargo test -p api user_dto", "files": ["src/dto/"] },
        { "key": "parse", "text": "parse the CSV", "after": ["row"], "files": ["src"] },
        { "key": "errors", "text": "show the importer errors", "after": ["dto", "parse"],
          "accept": "a bad row names its line", "phase": "2" },
        { "key": "review", "text": "review the importer", "role": "reviewer",
          "after": ["errors"] },
        { "key": "docs", "text": "document the import", "role": "general", "confirm": true,
          "brief": "README, one section" },
        { "text": "tidy up", "after": ["review", "docs"] }
    ]})
}

#[tokio::test]
async fn add_makes_a_whole_plan_in_one_call() {
    let f = fixture("plan_add").await;
    let answer = f.call(seven()).await.unwrap();
    let plan = f.state.plan(LEAD).await.unwrap();
    assert_eq!(plan.len(), 7);
    let shorts: Vec<String> = plan
        .iter()
        .map(|task| format!("#{}", short_task_id(&task.task_id)))
        .collect();
    assert_eq!(answer, format!("added 7 tasks: {}", shorts.join(" ")));
    assert_eq!(answer.len(), 84);
    let id = |index: usize| plan[index].task_id.clone();
    assert_eq!(plan[1].depends_on, [id(0)]);
    assert_eq!(plan[3].depends_on, [id(1), id(2)]);
    assert_eq!(plan[6].depends_on, [id(4), id(5)]);
    assert_eq!(plan[4].role.as_deref(), Some("reviewer"));
    assert_eq!(plan[5].role, None);
    assert_eq!(plan[1].files.entries(), ["src/dto/"]);
    // An existing folder named without its `/` gets it, so it holds what is inside.
    assert_eq!(plan[2].files.entries(), ["src/"]);
    assert_eq!(
        plan[1].gate,
        Some(GoalCheck::Command {
            command: "cargo test -p api user_dto".into()
        })
    );
    assert_eq!(plan[5].gate, Some(GoalCheck::UserConfirmation));
    let ready = f.state.ready_tasks(LEAD).await.unwrap();
    let ready: Vec<&str> = ready.iter().map(|task| task.task_id.as_str()).collect();
    assert_eq!(ready, [id(0).as_str(), id(5).as_str()]);
}

#[tokio::test]
async fn add_run_again_does_not_make_the_plan_twice() {
    let f = fixture("plan_replay").await;
    let first = f.call(seven()).await.unwrap();
    let again = f.call(seven()).await.unwrap();
    assert_eq!(again, first);
    assert_eq!(f.ids().await.len(), 7);
    assert!(f.tasks.repeatable(&seven()));
    // One task of a new add written again is refused, and nothing is added.
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "text": "a new one" },
            { "key": "x", "text": "tidy up" }] }))
        .await;
    assert!(refused.starts_with("task 2 is the same as #"), "{refused}");
    assert_eq!(f.ids().await.len(), 7);
}

#[tokio::test]
async fn a_loop_and_an_unknown_dependency_are_refused_by_name() {
    let f = fixture("plan_loop").await;
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "key": "a", "text": "one", "after": ["b"] },
            { "key": "b", "text": "two", "after": ["a"] }] }))
        .await;
    assert_eq!(
        refused,
        "task #a depends on #b, which depends on #a; take one of them out of `after`"
    );
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "key": "a", "text": "one", "after": ["a"] }] }))
        .await;
    assert!(
        refused.starts_with("task #a depends on itself"),
        "{refused}"
    );
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "key": "a", "text": "one", "after": ["nobody"] }] }))
        .await;
    assert_eq!(
        refused,
        "nobody is neither a key of this add nor a task of the plan; `after` takes keys of \
         this add, or ids that show prints"
    );
    assert!(f.ids().await.is_empty());
}

#[tokio::test]
async fn files_must_stay_inside_the_project() {
    let f = fixture("plan_files").await;
    let outside = f.root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, f.root.join("project/link")).unwrap();
    let mut entries = vec![
        "../outside/x.rs",
        "src/../../outside",
        "/etc/passwd",
        "src/*.rs",
        "src/..",
    ];
    if cfg!(unix) {
        entries.extend(["link/x.rs", "link/", "link"]);
    }
    for entry in entries {
        let refused = f
            .refused(json!({ "action": "add", "tasks": [
                { "key": "k", "text": "one", "files": [entry] }] }))
            .await;
        assert!(refused.starts_with("task k: "), "{entry}: {refused}");
        assert!(
            refused.contains("inside the project")
                || refused.contains("relative to the project")
                || refused.contains("not patterns"),
            "{entry}: {refused}"
        );
    }
    let many: Vec<String> = (0..17).map(|n| format!("src/{n}.rs")).collect();
    let refused = f
        .refused(json!({ "action": "add", "tasks": [{ "text": "one", "files": many }] }))
        .await;
    assert!(refused.contains("more than 16 files"), "{refused}");
    assert!(f.ids().await.is_empty());
    // A file that does not exist yet is fine.
    f.call(json!({ "action": "add", "tasks": [{ "text": "one", "files": ["src/new.rs"] }] }))
        .await
        .unwrap();
}

#[tokio::test]
async fn text_over_its_cap_a_bad_gate_and_unknown_fields_are_refused() {
    let f = fixture("plan_caps").await;
    let long = "x".repeat(601);
    let refused = f
        .refused(json!({ "action": "add", "tasks": [{ "text": long }] }))
        .await;
    assert_eq!(
        refused,
        "the task's text is longer than 600 characters; write it shorter"
    );
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "text": "one", "brief": "b".repeat(1201) }] }))
        .await;
    assert!(refused.contains("1200 characters"), "{refused}");
    for gate in ["  ".to_owned(), "x".repeat(16 * 1024 + 1)] {
        let refused = f
            .refused(json!({ "action": "add", "tasks": [{ "text": "one", "gate": gate }] }))
            .await;
        assert!(refused.contains("exact command"), "{refused}");
    }
    let refused = f
        .refused(json!({ "action": "add", "tasks": [
            { "text": "one", "gate": "make", "confirm": true }] }))
        .await;
    assert!(refused.contains("not both"), "{refused}");
    let refused = f
        .refused(json!({ "action": "add", "tasks": [{ "text": "one", "kind": "tdd" }] }))
        .await;
    assert!(refused.contains("procedures"), "{refused}");
    // No JSON shape panics; each comes back as an error.
    for wrong in [
        json!({ "action": "add", "tasks": "all of it" }),
        json!({ "action": "add", "tasks": [{ "text": 7 }] }),
        json!({ "action": "add", "tasks": [{ "text": "one", "owner": "m0" }] }),
        json!({ "action": "add", "tasks": [{ "text": "one" }], "id": "x" }),
        json!({ "action": "add", "tasks": [] }),
        json!({ "action": "run", "budget": { "steps": -1 } }),
        json!({ "action": "run", "budget": { "steps": 1e30 } }),
        json!({ "action": "run", "budget": { "steps": 0 } }),
        json!({ "action": "review", "ids": [null] }),
        json!({ "action": "review", "ids": [] }),
        json!({ "action": "drop" }),
        json!({ "action": ["add"] }),
        json!({}),
    ] {
        assert!(f.call(wrong).await.is_err());
    }
    assert!(f.ids().await.is_empty());
}

#[tokio::test]
async fn an_unknown_role_is_looked_for_once_then_refused_with_the_known_ones() {
    let f = fixture("plan_roles").await;
    // `reviewer` is on disk but not read yet: the add reads the catalog and takes it.
    f.call(json!({ "action": "add", "tasks": [{ "text": "one", "role": "reviewer" }] }))
        .await
        .unwrap();
    let refused = f
        .refused(json!({ "action": "add", "tasks": [{ "text": "two", "role": "tester" }] }))
        .await;
    assert!(
        refused.starts_with("no agent tester; roles are: general, reviewer."),
        "{refused}"
    );
    write_agent(&f.root, "tester");
    f.call(json!({ "action": "add", "tasks": [{ "text": "two", "role": "tester" }] }))
        .await
        .unwrap();
    let off = ToolContext {
        config: tool_context_with(&f.context.scope, &format!("{ON}agents:\n  tester: false\n"))
            .config,
        ..f.context.clone()
    };
    let refused = f
        .tasks
        .call(
            json!({ "action": "add", "tasks": [{ "text": "three", "role": "tester" }] }),
            off,
        )
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").starts_with("no agent tester"));
}

#[tokio::test]
async fn show_prints_the_plan_in_lines_within_its_caps() {
    let f = fixture("plan_show").await;
    f.call(json!({ "action": "add", "tasks": [
        { "key": "a", "text": "add the importer row type" },
        { "key": "b", "text": "make `email` optional in UserDto", "after": ["a"] },
        { "key": "c", "text": "review the importer", "role": "reviewer" },
        { "key": "d", "text": "show the importer errors", "after": ["b"] }] }))
        .await
        .unwrap();
    let ids = f.ids().await;
    f.drive(&ids[0], "m0", PlanStatus::Done).await;
    f.drive(&ids[1], "m1", PlanStatus::Running).await;
    f.drive(&ids[2], "m2", PlanStatus::Review).await;
    let s = |index: usize| short_task_id(&ids[index]).to_owned();
    let shown = f.call(json!({ "action": "show" })).await.unwrap();
    let expected = format!(
        "Plan: 1 of 4 done, 1 running, 1 in review, 1 open.\n\
         [>] #{} make `email` optional in UserDto (general, Ember-1) after {}\n\
         [?] #{} review the importer (reviewer, Ember-2) waits for you\n\
         [ ] #{} show the importer errors (general) after {}\n\
         [x] #{} add the importer row type (general)",
        s(1),
        s(0),
        s(2),
        s(3),
        s(1),
        s(0)
    );
    assert_eq!(shown, expected);
    println!("show of four: {} bytes", shown.len());
    assert!(f.tasks.read_only(&json!({ "action": "show" })));

    let one = f
        .call(json!({ "action": "show", "id": s(1) }))
        .await
        .unwrap();
    assert!(
        one.starts_with(&format!(
            "#{} running: make `email` optional in UserDto\nRole: general, Ember-1. \
             Attempts: 1",
            s(1)
        )),
        "{one}"
    );
    assert!(one.contains(&format!("After: #{} (done)", s(0))), "{one}");

    let big = fixture("plan_show_big").await;
    for batch in 0..2 {
        let tasks: Vec<Value> = (0..32)
            .map(|n| json!({ "text": format!("{} task {batch}-{n}", "long words ".repeat(12)) }))
            .collect();
        big.call(json!({ "action": "add", "tasks": tasks }))
            .await
            .unwrap();
    }
    let shown = big.call(json!({ "action": "show" })).await.unwrap();
    assert!(shown.len() <= text::MAX_BYTES, "{}", shown.len());
    assert!(shown.starts_with("Plan: 0 of 64 done, 64 open.\n"));
    let last = shown.lines().last().unwrap();
    assert!(
        last.starts_with("… ") && last.ends_with("more not shown; show with an id prints one"),
        "{last}"
    );
    // A text is cut on its line, never in the store.
    assert!(shown.lines().nth(1).unwrap().contains('…'));

    let short = fixture("plan_show_lines").await;
    for batch in 0..2 {
        let tasks: Vec<Value> = (0..32)
            .map(|n| json!({ "text": format!("t{batch}-{n}") }))
            .collect();
        short
            .call(json!({ "action": "add", "tasks": tasks }))
            .await
            .unwrap();
    }
    let shown = short.call(json!({ "action": "show" })).await.unwrap();
    assert_eq!(shown.lines().count(), 1 + text::MAX_LINES + 1);
    assert!(shown.ends_with("… 4 more not shown; show with an id prints one"));
}

#[tokio::test]
async fn review_takes_tasks_in_review_and_answers_a_repeat_as_before() {
    let f = fixture("plan_review").await;
    f.call(json!({ "action": "add", "tasks": [
        { "text": "one" }, { "text": "two" }, { "text": "three" }] }))
        .await
        .unwrap();
    let ids = f.ids().await;
    let s = |index: usize| short_task_id(&ids[index]).to_owned();
    f.drive(&ids[0], "m0", PlanStatus::Review).await;
    f.drive(&ids[1], "m1", PlanStatus::Review).await;

    let accept = json!({ "action": "review", "ids": [s(0)] });
    assert!(f.tasks.repeatable(&accept));
    let answer = f.call(accept.clone()).await.unwrap();
    assert_eq!(answer, format!("accepted #{}", s(0)));
    assert_eq!(f.call(accept).await.unwrap(), answer);

    let reject = json!({ "action": "review", "id": s(1), "text": "the test still fails" });
    let answer = f.call(reject.clone()).await.unwrap();
    assert_eq!(answer, format!("sent back #{} to redo (1 of 2)", s(1)));
    assert_eq!(f.call(reject).await.unwrap(), answer);
    let task = f.state.plan_task(LEAD, &ids[1]).await.unwrap();
    assert_eq!((task.status, task.reworks), (PlanStatus::Running, 1));

    let refused = f
        .refused(json!({ "action": "review", "ids": [s(2)] }))
        .await;
    assert_eq!(
        refused,
        format!(
            "task #{} is open; review takes a task in review, which show marks [?]",
            s(2)
        )
    );
    let refused = f
        .refused(json!({ "action": "review", "id": s(2), "text": "redo" }))
        .await;
    assert!(
        refused.contains("review takes a task in review"),
        "{refused}"
    );
    let refused = f.refused(json!({ "action": "review", "id": s(2) })).await;
    assert!(refused.starts_with("review accepts `ids`"), "{refused}");
}

#[tokio::test]
async fn fail_and_reopen_repeated_change_nothing_more() {
    let f = fixture("plan_fail").await;
    f.call(json!({ "action": "add", "tasks": [
        { "key": "a", "text": "one" }, { "text": "two", "after": ["a"] }] }))
        .await
        .unwrap();
    let ids = f.ids().await;
    let id = short_task_id(&ids[0]).to_owned();
    let fail = json!({ "action": "fail", "id": id, "text": "wrong approach" });
    assert!(f.tasks.repeatable(&fail));
    let answer = f.call(fail.clone()).await.unwrap();
    assert_eq!(answer, format!("failed #{id}"));
    assert_eq!(f.call(fail).await.unwrap(), answer);
    let behind = f.state.plan_task(LEAD, &ids[1]).await.unwrap();
    assert_eq!(behind.status, PlanStatus::Blocked);

    let reopen = json!({ "action": "reopen", "id": id });
    let answer = f.call(reopen.clone()).await.unwrap();
    assert_eq!(answer, format!("reopened #{id}"));
    assert_eq!(f.call(reopen).await.unwrap(), answer);
    let behind = f.state.plan_task(LEAD, &ids[1]).await.unwrap();
    assert_eq!(behind.status, PlanStatus::Open);

    let edit = json!({ "action": "edit", "id": id, "tasks": [{ "text": "one, better" }] });
    assert_eq!(f.call(edit).await.unwrap(), format!("edited #{id}"));
    f.drive(&ids[0], "m0", PlanStatus::Running).await;
    let refused = f
        .refused(json!({ "action": "edit", "id": id, "text": "late" }))
        .await;
    assert!(
        refused.contains("edit changes only a task not yet started"),
        "{refused}"
    );
}

#[tokio::test]
async fn the_board_works_as_before_and_leaves_given_out_tasks_alone() {
    let main = fixture_with("plan_board", "limits:\n  plan_graph: true\n").await;
    let board = main
        .call(json!({ "action": "add", "task": "port", "items": ["read"] }))
        .await
        .unwrap();
    assert!(board.contains("port #"), "{board}");
    let id = main.state.thread_tasks(LEAD).await.unwrap()[0]
        .task_id
        .clone();
    main.call(json!({ "action": "add_items", "id": id, "items": ["write"] }))
        .await
        .unwrap();
    let board = main
        .call(json!({ "action": "done", "id": id }))
        .await
        .unwrap();
    assert!(board.starts_with("Done\n  [x] port"), "{board}");
    for plan_only in [
        json!({ "action": "run" }),
        json!({ "action": "add", "tasks": [{ "text": "x" }] }),
    ] {
        let refused = main.refused(plan_only).await;
        assert!(
            refused.starts_with("the plan's actions need TEAM mode"),
            "{refused}"
        );
    }

    let f = fixture("plan_board_given").await;
    f.call(json!({ "action": "add", "tasks": [{ "text": "one" }, { "text": "two" }] }))
        .await
        .unwrap();
    let ids = f.ids().await;
    f.drive(&ids[0], "m0", PlanStatus::Running).await;
    for arguments in [
        json!({ "action": "done", "id": ids[0] }),
        json!({ "action": "add_items", "id": ids[0], "items": ["more"] }),
        json!({ "action": "start", "id": ids[0] }),
    ] {
        let refused = f.refused(arguments).await;
        assert!(
            refused.ends_with("is running in the team's plan; the board cannot change it"),
            "{refused}"
        );
    }
    // The same rule in MAIN mode, where only the board is offered.
    let main_context = ToolContext {
        config: tool_context_with(&f.context.scope, "").config,
        ..f.context.clone()
    };
    let refused = f
        .tasks
        .call(json!({ "action": "done", "id": ids[0] }), main_context)
        .await
        .unwrap_err();
    assert!(format!("{refused:#}").contains("the board cannot change it"));
    // A task not given out is still the board's to finish.
    f.call(json!({ "action": "done", "id": ids[1] }))
        .await
        .unwrap();
    assert_eq!(
        f.state.plan_task(LEAD, &ids[1]).await.unwrap().status,
        PlanStatus::Done
    );
}

/// A team runner that remembers what it was asked.
#[derive(Default)]
struct Recorder {
    runs: Mutex<Vec<TeamBudget>>,
    stops: Mutex<usize>,
}

impl TeamRunner for Recorder {
    fn run<'a>(
        &'a self,
        _lead: &'a str,
        budget: TeamBudget,
        _progress: Option<crate::tools::Progress>,
    ) -> BoxFuture<'a, super::team::Started> {
        self.runs.lock().push(budget);
        Box::pin(async {
            Ok(super::team::Started {
                given: 2,
                gates: super::team::GatesYes::NotAsked,
            })
        })
    }

    fn stop<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, ()> {
        *self.stops.lock() += 1;
        Box::pin(async { Ok(()) })
    }

    fn team<'a>(&'a self, _lead: &'a str) -> BoxFuture<'a, Option<Team>> {
        Box::pin(async { Ok(None) })
    }
}

#[tokio::test]
async fn run_and_stop_reach_the_team_runner() {
    let f = fixture("plan_run").await;
    let refused = f.refused(json!({ "action": "run" })).await;
    assert!(
        refused.starts_with("the plan has nothing left to do"),
        "{refused}"
    );
    f.call(json!({ "action": "add", "tasks": [{ "text": "one" }] }))
        .await
        .unwrap();
    let refused = f.refused(json!({ "action": "run" })).await;
    assert!(
        refused.starts_with("the team runner is not built yet"),
        "{refused}"
    );
    let refused = f.refused(json!({ "action": "stop" })).await;
    assert!(
        refused.starts_with("the team runner is not built yet"),
        "{refused}"
    );

    let recorder = Arc::new(Recorder::default());
    let tasks = Tasks::new(Arc::clone(&f.state), LEAD.into())
        .lead(Agents::new(f.root.join("agents"), Arc::clone(&f.state)))
        .with_team(Arc::clone(&recorder) as Arc<dyn TeamRunner>);
    let run = json!({ "action": "run", "budget": { "steps": 50 } });
    assert!(tasks.repeatable(&run));
    let answer = tasks.call(run, f.context.clone()).await.unwrap();
    assert_eq!(answer, "running: 2 given out");
    assert_eq!(
        recorder.runs.lock().as_slice(),
        [TeamBudget::new(3, 50, 4_000_000, 120).unwrap()]
    );
    let refused = tasks
        .call(
            json!({ "action": "run", "budget": { "steps": 401 } }),
            f.context.clone(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        format!("{refused:#}"),
        "budget.steps 401 is above the ceiling 400; give 400 or less, or ask the user to \
         raise it in the settings"
    );
    let answer = tasks
        .call(json!({ "action": "stop" }), f.context.clone())
        .await
        .unwrap();
    assert_eq!(answer, "stopped");
    assert_eq!(*recorder.stops.lock(), 1);
}
