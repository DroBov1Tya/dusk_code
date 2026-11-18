//! The plan and team requests: what each answers and refuses, the page of a long plan,
//! the mapping of every field, that a task of another chat is never reached, and who
//! hears of a change.

use super::*;
use crate::{
    client::{Area, Scope},
    config::Limits,
    init::paths::Paths,
    state::{BlockedOn, Claim, NewTask, PassedBy, TaskFiles, TeamCeilings},
    testing::temp_dir,
};
use dusk_protocol::{
    client::{Request, ThreadRequest, WorkRequest},
    thread::{
        GoalCheck, PLAN_PAGE_DONE, PlanPage, Task, TaskStatus, Team, TeamBudget, TeamEnd,
        TeamStatus, ThreadEvent, WaitsOn,
    },
};
use tokio::sync::mpsc;

const NOW: i64 = 5_000_000;

struct Rig {
    server: Server,
    subscribers: Subscribers,
}

impl Rig {
    /// A chat `lead` with members `m0` (named Ada) and `m1`, and another chat `other`.
    async fn new(name: &str) -> Self {
        let root = temp_dir(name);
        let paths = Paths::new(root.join("config"), root.join("work"));
        let runtime = Runtime::start(paths, Some(root.join("state.db")))
            .await
            .unwrap();
        let state = runtime.state();
        state.create_thread("lead", "test").await.unwrap();
        for (id, name) in [("m0", "Ada"), ("m1", "Bob")] {
            state
                .create_agent_thread(id, "lead", "general", name)
                .await
                .unwrap();
        }
        state.create_thread("other", "test").await.unwrap();
        let subscribers = Subscribers::default();
        let (mut server, _) = Server::new(runtime, Arc::clone(&subscribers));
        server.caller = 1;
        Self {
            server,
            subscribers,
        }
    }

    fn state(&self) -> &crate::state::State {
        self.server.runtime.state()
    }

    /// Attaches a client; `chats` binds it to those chats, none lets it see all.
    fn attach(&mut self, client: ClientId, chats: Option<&str>) -> mpsc::UnboundedReceiver<Event> {
        let scope = match chats {
            None => Scope::full(),
            Some(chat) => Scope::threads(
                [chat.to_owned()],
                &[Area::ThreadRead, Area::ThreadWrite, Area::ThreadApprovals],
            ),
        };
        let (queue, events) = mpsc::unbounded_channel();
        self.subscribers.lock().push(Subscriber {
            client,
            scope: scope.clone(),
            queue,
        });
        self.server.clients.insert(
            client,
            Attached {
                source: "test".into(),
                scope,
            },
        );
        events
    }

    async fn ask(&mut self, request: WorkRequest) -> Result<Response, String> {
        self.server
            .work(request)
            .await
            .map_err(|error| format!("{error:#}"))
    }

    async fn page(&mut self, thread: &str, before: Option<&str>, limit: u32) -> PlanPage {
        let request = WorkRequest::PlanTasks {
            thread_id: thread.into(),
            done_before: before.map(str::to_owned),
            limit,
        };
        match self.ask(request).await.unwrap() {
            Response::PlanPage(page) => page,
            other => panic!("not a page: {other:?}"),
        }
    }

    async fn add(&self, chat: &str, tasks: &[NewTask]) -> Vec<String> {
        let added = self.state().add_plan_tasks(chat, tasks).await.unwrap();
        added.into_iter().map(|task| task.task_id).collect()
    }

    /// Takes `task` to review on `member`'s turn.
    async fn to_review(&self, task: &str, member: &str) -> String {
        let state = self.state();
        let claim = Claim {
            owner: member,
            at_once: 8,
            model: Some("model-a"),
            now: NOW,
        };
        state.claim_task("lead", task, claim).await.unwrap();
        let turn_id = format!("turn-{task}");
        sqlx::query(
            "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at)
             VALUES (?, ?, 1, 'interrupted', 0, 0)",
        )
        .bind(&turn_id)
        .bind(member)
        .execute(&state.pool)
        .await
        .unwrap();
        state.start_task(member, task, &turn_id).await.unwrap();
        state.hand_in(member, task).await.unwrap();
        turn_id
    }
}

fn written(text: &str, gate: Option<GoalCheck>) -> NewTask {
    NewTask {
        description: text.into(),
        gate,
        ..NewTask::default()
    }
}

fn budget(steps: u32) -> TeamBudget {
    TeamBudget::new(3, steps, 4_000_000, 120).unwrap()
}

async fn start_team(rig: &Rig, steps: u32) {
    let ceilings = TeamCeilings::from_limits(&Limits::default()).unwrap();
    let now = crate::state::timestamp_ms().unwrap();
    rig.state()
        .start_team("lead", budget(steps), ceilings, now)
        .await
        .unwrap();
}

fn drained(events: &mut mpsc::UnboundedReceiver<Event>) -> Vec<ThreadEvent> {
    std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            Event::Thread { event, .. } => Some(*event),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_task_is_shown_with_every_field_the_store_has() {
    let mut rig = Rig::new("plan_serve_mapping").await;
    let files = TaskFiles::new(["src/", "Cargo.toml"]).unwrap();
    let first = NewTask {
        key: Some("a".into()),
        description: "write it".into(),
        role: Some("reviewer".into()),
        phase: "build".into(),
        acceptance: "it compiles".into(),
        brief: "start with the parser".into(),
        gate: Some(GoalCheck::UserConfirmation),
        files,
        ..NewTask::default()
    };
    let second = NewTask {
        after: vec!["a".into()],
        ..written("then test it", None)
    };
    let ids = rig.add("lead", &[first, second]).await;
    let turn = rig.to_review(&ids[0], "m0").await;
    let state = rig.state();
    for (purpose, prompt, completion) in
        [("answer", 100, 20), ("answer", 200, 30), ("compact", 50, 5)]
    {
        sqlx::query(
            "INSERT INTO model_usage (thread_id, turn_id, purpose, model, through_sequence,
                 prompt_tokens, cached_prompt_tokens, completion_tokens, reasoning_tokens,
                 created_at)
             VALUES ('m0', ?, ?, 'model-a', 1, ?, 0, ?, 0, 1)",
        )
        .bind(&turn)
        .bind(purpose)
        .bind(prompt)
        .bind(completion)
        .execute(&state.pool)
        .await
        .unwrap();
    }
    // The same file changed twice counts once.
    for (id, path) in [("c1", "src/a.rs"), ("c2", "src/a.rs"), ("c3", "src/b.rs")] {
        sqlx::query(
            "INSERT INTO changes (change_id, thread_id, turn_id, project, path, kind,
                 size_after, created_at) VALUES (?, 'm0', ?, '/p', ?, 'created', 1, 0)",
        )
        .bind(id)
        .bind(&turn)
        .bind(path)
        .execute(&state.pool)
        .await
        .unwrap();
    }

    // A member reads the plan of its chat.
    let page = rig.page("m1", None, 10).await;
    assert_eq!(page.tasks.len(), 2);
    let shown = &page.tasks[0];
    assert_eq!(shown.task_id, ids[0]);
    assert_eq!(shown.status, TaskStatus::Review);
    assert!(!shown.done);
    assert_eq!(shown.role.as_deref(), Some("reviewer"));
    assert_eq!(shown.phase, "build");
    assert_eq!(shown.acceptance, "it compiles");
    assert_eq!(shown.brief, "start with the parser");
    assert_eq!(shown.gate, Some(GoalCheck::UserConfirmation));
    assert_eq!(shown.files, ["src/", "Cargo.toml"]);
    let owner = shown.owner.as_ref().unwrap();
    assert_eq!(
        (owner.thread_id.as_str(), owner.name.as_str()),
        ("m0", "Ada")
    );
    assert_eq!((shown.attempts, shown.reworks), (1, 0));
    assert_eq!(shown.model.as_deref(), Some("model-a"));
    assert_eq!((shown.steps, shown.tokens, shown.changes), (2, 405, 2));
    assert_eq!(page.tasks[1].depends_on, [ids[0].clone()]);
    assert_eq!(page.tasks[1].status, TaskStatus::Open);
    assert!(page.tasks[1].owner.is_none());

    // The board's request shows the same tasks.
    let board = rig.server.runtime.thread_tasks("lead").await.unwrap();
    assert_eq!(board, page.tasks);
    let Response::Tasks(over_the_wire) = rig
        .server
        .thread(ThreadRequest::Tasks {
            thread_id: "lead".into(),
        })
        .await
        .unwrap()
    else {
        panic!("not tasks");
    };
    assert_eq!(over_the_wire, board);
}

#[tokio::test]
async fn what_a_task_waits_for_is_shown() {
    let mut rig = Rig::new("plan_serve_waits").await;
    let ids = rig
        .add("lead", &[written("one", None), written("two", None)])
        .await;
    rig.to_review(&ids[0], "m0").await;
    rig.state()
        .hold_for_lead("lead", &ids[0], "touched more than its files")
        .await
        .unwrap();
    let claim = Claim {
        owner: "m1",
        at_once: 8,
        model: None,
        now: NOW,
    };
    rig.state()
        .claim_task("lead", &ids[1], claim)
        .await
        .unwrap();
    let turn = "turn-two";
    sqlx::query(
        "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at, finished_at)
         VALUES (?, 'm1', 1, 'interrupted', 0, 0)",
    )
    .bind(turn)
    .execute(&rig.state().pool)
    .await
    .unwrap();
    rig.state().start_task("m1", &ids[1], turn).await.unwrap();
    rig.state()
        .block_task("m1", &ids[1], Some(&BlockedOn::Message(7)), "need x")
        .await
        .unwrap();

    let page = rig.page("lead", None, 10).await;
    assert_eq!(page.tasks[0].waits_on, Some(WaitsOn::Lead));
    assert_eq!(page.tasks[0].note, "touched more than its files");
    assert_eq!(page.tasks[1].waits_on, Some(WaitsOn::Message(7)));
    assert_eq!(page.tasks[1].status, TaskStatus::Blocked);
    assert_eq!(page.counts.review, 1);
    assert_eq!(page.counts.blocked, 1);
}

#[tokio::test]
async fn a_long_plan_is_read_in_pages_of_done_tasks() {
    let mut rig = Rig::new("plan_serve_paging").await;
    rig.add("lead", &[written("open one", None)]).await;
    for number in 0..250 {
        sqlx::query(
            "INSERT INTO tasks (task_id, thread_id, description, status, done, created_at,
                 updated_at) VALUES (?, 'lead', 'finished', 'done', 1, ?, ?)",
        )
        .bind(format!("done-{number:04}"))
        .bind(1_000 + number)
        .bind(1_000 + number)
        .execute(&rig.state().pool)
        .await
        .unwrap();
    }
    // Asking for more than a page, or for none, is held to the page's bounds.
    let first = rig.page("lead", None, 5_000).await;
    let done_of = |page: &PlanPage| -> Vec<String> {
        page.tasks
            .iter()
            .filter(|task| task.done)
            .map(|task| task.task_id.clone())
            .collect()
    };
    assert_eq!(done_of(&first).len(), PLAN_PAGE_DONE as usize);
    assert_eq!(first.counts.done, 250);
    assert_eq!(first.counts.open, 1);
    assert!(!first.tasks[0].done, "tasks not done come first");

    let second = rig.page("lead", first.next.as_deref(), 100).await;
    let third = rig.page("lead", second.next.as_deref(), 100).await;
    assert_eq!(done_of(&second).len(), 100);
    assert_eq!(done_of(&third).len(), 50);
    assert!(third.next.is_none());
    // Not done tasks come on every page; the done ones, newest first, never repeat.
    assert!(!third.tasks[0].done);
    let all: Vec<String> = [&first, &second, &third]
        .into_iter()
        .flat_map(done_of)
        .collect();
    let mut newest_first = all.clone();
    newest_first.sort_by(|a, b| b.cmp(a));
    newest_first.dedup();
    assert_eq!(all, newest_first);
    assert_eq!(all.len(), 250);
    assert_eq!(all[0], "done-0249");

    assert_eq!(done_of(&rig.page("lead", None, 0).await).len(), 1);
}

#[tokio::test]
async fn the_team_is_read_from_the_chat_or_any_member() {
    let mut rig = Rig::new("plan_serve_team").await;
    let none = rig
        .ask(WorkRequest::Team {
            thread_id: "lead".into(),
        })
        .await;
    assert!(matches!(none, Ok(Response::Team(None))));
    start_team(&rig, 400).await;
    let ids = rig.add("lead", &[written("one", None)]).await;
    rig.to_review(&ids[0], "m0").await;
    for thread in ["lead", "m0"] {
        let Ok(Response::Team(Some(team))) = rig
            .ask(WorkRequest::Team {
                thread_id: thread.into(),
            })
            .await
        else {
            panic!("no team for {thread}");
        };
        assert_eq!(team.thread_id, "lead");
        assert_eq!(team.status, TeamStatus::Running);
        assert_eq!(team.members.len(), 1);
        assert_eq!(team.members[0].name, "Ada");
    }
    let other = rig
        .ask(WorkRequest::Team {
            thread_id: "other".into(),
        })
        .await;
    assert!(matches!(other, Ok(Response::Team(None))));
    let missing = rig
        .ask(WorkRequest::Team {
            thread_id: "ghost".into(),
        })
        .await;
    assert_eq!(missing.unwrap_err(), "no such thread");
}

#[tokio::test]
async fn a_user_confirms_only_a_task_in_review_that_waits_for_the_user() {
    let mut rig = Rig::new("plan_serve_confirm").await;
    let command = GoalCheck::Command {
        command: "cargo test".into(),
    };
    let ids = rig
        .add(
            "lead",
            &[
                written("open", Some(GoalCheck::UserConfirmation)),
                written("claimed", Some(GoalCheck::UserConfirmation)),
                written("no gate", None),
                written("command", Some(command)),
                written("confirm me", Some(GoalCheck::UserConfirmation)),
                written("finished", None),
            ],
        )
        .await;
    let claim = Claim {
        owner: "m1",
        at_once: 8,
        model: None,
        now: NOW,
    };
    rig.state()
        .claim_task("lead", &ids[1], claim)
        .await
        .unwrap();
    let mut events = rig.attach(2, None);
    let confirm = |task: &str| WorkRequest::ConfirmTask {
        thread_id: "lead".into(),
        task_id: task.into(),
    };

    // Open, at work, failed.
    for task in [&ids[0], &ids[1]] {
        let refused = rig.ask(confirm(task)).await.unwrap_err();
        assert!(refused.contains("it cannot become done"), "{refused}");
    }
    rig.state().fail_task("lead", &ids[5], "no").await.unwrap();
    let failed = rig.ask(confirm(&ids[5])).await.unwrap_err();
    assert!(failed.contains("it cannot become done"), "{failed}");

    // In review, but its gate is not the user's: none, then a command.
    rig.to_review(&ids[2], "m0").await;
    let no_gate = rig.ask(confirm(&ids[2])).await.unwrap_err();
    assert!(no_gate.contains("not the user's confirmation"), "{no_gate}");
    rig.state()
        .pass_task("lead", &ids[2], PassedBy::Lead)
        .await
        .unwrap();
    rig.to_review(&ids[3], "m0").await;
    let command = rig.ask(confirm(&ids[3])).await.unwrap_err();
    assert!(command.contains("not the user's confirmation"), "{command}");
    rig.state()
        .pass_task("lead", &ids[3], PassedBy::Gate)
        .await
        .unwrap();
    assert!(drained(&mut events).is_empty(), "a refusal changes nothing");

    // The user's gate: a task in review passes, once.
    rig.to_review(&ids[4], "m0").await;
    assert!(matches!(
        rig.ask(confirm(&ids[4])).await,
        Ok(Response::Done)
    ));
    let done = rig.state().plan_task_view("lead", &ids[4]).await.unwrap();
    assert_eq!(done.status, TaskStatus::Done);
    let heard = drained(&mut events);
    assert!(matches!(
        heard.as_slice(),
        [ThreadEvent::TaskChanged(Task {
            status: TaskStatus::Done,
            ..
        })]
    ));
    let again = rig.ask(confirm(&ids[4])).await.unwrap_err();
    assert!(again.contains("it cannot become done"), "{again}");
}

#[tokio::test]
async fn a_task_of_another_chat_is_never_found() {
    let mut rig = Rig::new("plan_serve_cross").await;
    let mine = rig
        .add(
            "lead",
            &[written("mine", Some(GoalCheck::UserConfirmation))],
        )
        .await;
    let theirs = rig
        .add(
            "other",
            &[written("theirs", Some(GoalCheck::UserConfirmation))],
        )
        .await;
    let state = rig.state();
    sqlx::query("UPDATE tasks SET status = 'done', done = 1 WHERE task_id = ?")
        .bind(&theirs[0])
        .execute(&state.pool)
        .await
        .unwrap();

    let confirm = rig
        .ask(WorkRequest::ConfirmTask {
            thread_id: "lead".into(),
            task_id: theirs[0].clone(),
        })
        .await
        .unwrap_err();
    assert!(confirm.contains("no task"), "{confirm}");
    let cursor = rig
        .ask(WorkRequest::PlanTasks {
            thread_id: "lead".into(),
            done_before: Some(theirs[0].clone()),
            limit: 10,
        })
        .await
        .unwrap_err();
    assert!(cursor.contains("no task"), "{cursor}");
    let page = rig.page("lead", None, 10).await;
    assert_eq!(page.tasks.len(), 1);
    assert_eq!(page.tasks[0].task_id, mine[0]);
    assert_eq!(page.counts.done, 0);
}

#[tokio::test]
async fn the_user_raises_a_budget_only_to_the_ceiling() {
    let mut rig = Rig::new("plan_serve_budget").await;
    let set = |steps: u32| WorkRequest::SetTeamBudget {
        thread_id: "lead".into(),
        budget: budget(steps),
    };
    let none = rig.ask(set(300)).await.unwrap_err();
    assert!(none.contains("no team has run"), "{none}");
    start_team(&rig, 200).await;
    let mut events = rig.attach(2, None);

    assert!(matches!(rig.ask(set(400)).await, Ok(Response::Done)));
    let Ok(Response::Team(Some(team))) = rig
        .ask(WorkRequest::Team {
            thread_id: "lead".into(),
        })
        .await
    else {
        panic!("no team");
    };
    assert_eq!(team.budget.steps(), 400);
    assert!(matches!(drained(&mut events).as_slice(), [ThreadEvent::Team(told)] if *told == team));

    let past = rig.ask(set(401)).await.unwrap_err();
    assert_eq!(past, "steps past the ceiling of 400");
    assert!(drained(&mut events).is_empty());
    rig.ask(WorkRequest::StopTeam {
        thread_id: "lead".into(),
    })
    .await
    .unwrap();
    let ended = rig.ask(set(100)).await.unwrap_err();
    assert!(ended.contains("not running"), "{ended}");
}

#[tokio::test]
async fn stopping_a_team_ends_it_and_gives_its_work_back() {
    let mut rig = Rig::new("plan_serve_stop").await;
    let stop = || WorkRequest::StopTeam {
        thread_id: "lead".into(),
    };
    assert!(
        rig.ask(stop())
            .await
            .unwrap_err()
            .contains("no team has run")
    );
    start_team(&rig, 400).await;
    let ids = rig.add("lead", &[written("working", None)]).await;
    let claim = Claim {
        owner: "m0",
        at_once: 8,
        model: None,
        now: NOW,
    };
    rig.state()
        .claim_task("lead", &ids[0], claim)
        .await
        .unwrap();
    let mut events = rig.attach(2, None);

    assert!(matches!(rig.ask(stop()).await, Ok(Response::Done)));
    let heard = drained(&mut events);
    let [ThreadEvent::TaskChanged(returned), ThreadEvent::Team(team)] = heard.as_slice() else {
        panic!("unexpected events: {heard:?}");
    };
    assert_eq!(returned.status, TaskStatus::Open);
    assert!(returned.owner.is_none());
    assert_eq!(team.status, TeamStatus::Ended(TeamEnd::Stopped));
    let again = rig.ask(stop()).await.unwrap_err();
    assert!(again.contains("not running"), "{again}");
}

#[tokio::test]
async fn only_clients_that_may_read_the_chat_hear_of_its_plan() {
    let mut rig = Rig::new("plan_serve_events").await;
    let mut full = rig.attach(2, None);
    let mut of_chat = rig.attach(3, Some("lead"));
    let mut of_other = rig.attach(4, Some("other"));
    start_team(&rig, 200).await;

    // A client bound to the chat sets the budget from one of its members' threads.
    rig.server.caller = 3;
    let request = Request::Work(WorkRequest::SetTeamBudget {
        thread_id: "m0".into(),
        budget: budget(300),
    });
    assert!(rig.server.admit(3, &request).await.is_ok());
    let Request::Work(request) = request else {
        unreachable!()
    };
    rig.server.work(request).await.unwrap();

    for events in [&mut full, &mut of_chat] {
        let heard = drained(events);
        assert!(
            matches!(heard.as_slice(), [ThreadEvent::Team(Team { thread_id, .. })] if thread_id == "lead")
        );
    }
    assert!(drained(&mut of_other).is_empty());

    // And it may not name the other chat's thread at all.
    let elsewhere = Request::Work(WorkRequest::StopTeam {
        thread_id: "other".into(),
    });
    assert_eq!(
        rig.server.admit(3, &elsewhere).await.unwrap_err(),
        "no such thread"
    );
}
