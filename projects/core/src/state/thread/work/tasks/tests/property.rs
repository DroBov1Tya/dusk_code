//! Whatever moves are asked for, in whatever order, the plan stays whole: every refusal
//! is typed, and the rules below hold after each step.

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use sqlx::Row;

use super::kit::{LEAD, NOW, claim, team, turn};
use crate::state::{
    BlockedOn, Claim, LEASE_MS, MAX_ATTEMPTS, MAX_REWORKS, NewTask, PassedBy, PlanError,
    PlanStatus, PlanTask, State, TaskEdit, TaskFiles, overlaps,
};

const FILES: [&str; 6] = [
    "src/",
    "src/a.rs",
    "src/a/",
    "src/a/b.rs",
    "docs/x.md",
    "src/ab",
];

#[derive(Debug, Clone)]
enum Step {
    /// New tasks: for each, which earlier tasks it depends on and which files it has.
    Add(Vec<(Vec<usize>, Vec<usize>)>),
    EditAfter(usize, Vec<usize>),
    Claim(usize, usize, u32),
    Start(usize),
    Renew(usize),
    Release(usize),
    Expire(i64),
    ReturnWork,
    HandIn(usize),
    Block(usize, Option<usize>),
    Unblock(usize),
    Hold(usize),
    Pass(usize, u8),
    Rework(usize),
    Fail(usize),
    Reopen(usize),
    BoardComplete(usize),
    BoardAddItems(usize),
}

fn step() -> impl Strategy<Value = Step> {
    let task = 0..12_usize;
    let picks = || prop::collection::vec(0..12_usize, 0..3);
    prop_oneof![
        2 => prop::collection::vec((picks(), prop::collection::vec(0..FILES.len(), 0..3)), 1..4)
            .prop_map(Step::Add),
        1 => (task.clone(), picks()).prop_map(|(task, after)| Step::EditAfter(task, after)),
        6 => (task.clone(), 0..4_usize, 1..4_u32)
            .prop_map(|(task, member, at_once)| Step::Claim(task, member, at_once)),
        4 => task.clone().prop_map(Step::Start),
        1 => task.clone().prop_map(Step::Renew),
        2 => task.clone().prop_map(Step::Release),
        1 => (0..3_i64).prop_map(Step::Expire),
        1 => Just(Step::ReturnWork),
        4 => task.clone().prop_map(Step::HandIn),
        1 => (task.clone(), prop::option::of(0..12_usize))
            .prop_map(|(task, on)| Step::Block(task, on)),
        1 => task.clone().prop_map(Step::Unblock),
        1 => task.clone().prop_map(Step::Hold),
        4 => (task.clone(), 0..3_u8).prop_map(|(task, by)| Step::Pass(task, by)),
        2 => task.clone().prop_map(Step::Rework),
        1 => task.clone().prop_map(Step::Fail),
        1 => task.clone().prop_map(Step::Reopen),
        1 => task.clone().prop_map(Step::BoardComplete),
        1 => task.prop_map(Step::BoardAddItems),
    ]
}

/// The id at `index` of the plan, wrapped around; a text no task has when it is empty.
fn pick(ids: &[String], index: usize) -> String {
    if ids.is_empty() {
        return "no-such-task".to_owned();
    }
    ids[index % ids.len()].clone()
}

/// Makes one step. `Ok` whether the store made the move or refused it; `Err` when the
/// refusal was not one of the store's own, which is a rule broken underneath.
async fn make(state: &State, ids: &mut Vec<String>, step: &Step) -> Result<(), String> {
    let owner_of = |plan: &[PlanTask], id: &str| {
        let owner = plan.iter().find(|task| task.task_id == id);
        owner
            .and_then(|task| task.owner_thread_id.clone())
            .unwrap_or_else(|| "m0".to_owned())
    };
    let plan = state.plan(LEAD).await.map_err(|error| error.to_string())?;
    let made: Result<(), PlanError> = match step {
        Step::Add(new) => {
            let tasks: Vec<NewTask> = new
                .iter()
                .enumerate()
                .map(|(at, (after, files))| NewTask {
                    key: Some(format!("k{at}")),
                    description: format!("task {}", ids.len() + at),
                    // A key of this add, later ones too, or a task of the plan.
                    after: after
                        .iter()
                        .map(|pick_at| match pick_at % 3 {
                            0 => format!("k{}", pick_at % new.len()),
                            _ => pick(ids, *pick_at),
                        })
                        .collect(),
                    files: TaskFiles::new(files.iter().map(|file| FILES[*file])).unwrap(),
                    ..NewTask::default()
                })
                .collect();
            match state.add_plan_tasks(LEAD, &tasks).await {
                Ok(added) => {
                    ids.extend(added.into_iter().map(|task| task.task_id));
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
        Step::EditAfter(task, after) => {
            let edit = TaskEdit {
                after: Some(after.iter().map(|at| pick(ids, *at)).collect()),
                ..TaskEdit::default()
            };
            state
                .edit_plan_task(LEAD, &pick(ids, *task), &edit)
                .await
                .map(drop)
        }
        Step::Claim(task, member, at_once) => {
            let owner = format!("m{member}");
            let claim = Claim {
                at_once: *at_once,
                ..claim(&owner)
            };
            state
                .claim_task(LEAD, &pick(ids, *task), claim)
                .await
                .map(drop)
        }
        Step::Start(task) => {
            let id = pick(ids, *task);
            let owner = owner_of(&plan, &id);
            let turn_id = turn(state, &owner).await;
            state.start_task(&owner, &id, &turn_id).await.map(drop)
        }
        Step::Renew(task) => {
            let id = pick(ids, *task);
            state.renew_lease(&owner_of(&plan, &id), &id, NOW + 1).await
        }
        Step::Release(task) => state
            .release_task(LEAD, &pick(ids, *task), "lost")
            .await
            .map(drop),
        Step::Expire(after) => state
            .release_expired(LEAD, NOW + LEASE_MS * after)
            .await
            .map(drop),
        Step::ReturnWork => state.return_work(LEAD).await.map(drop),
        Step::HandIn(task) => {
            let id = pick(ids, *task);
            state.hand_in(&owner_of(&plan, &id), &id).await.map(drop)
        }
        Step::Block(task, on) => {
            let id = pick(ids, *task);
            let on = on.map(|on| BlockedOn::Task(pick(ids, on)));
            state
                .block_task(&owner_of(&plan, &id), &id, on.as_ref(), "stuck")
                .await
                .map(drop)
        }
        Step::Unblock(task) => state
            .unblock_task(LEAD, &pick(ids, *task), NOW)
            .await
            .map(drop),
        Step::Hold(task) => state
            .hold_for_lead(LEAD, &pick(ids, *task), "x")
            .await
            .map(drop),
        Step::Pass(task, by) => {
            let by = [PassedBy::Lead, PassedBy::Gate, PassedBy::User][usize::from(*by)];
            state.pass_task(LEAD, &pick(ids, *task), by).await.map(drop)
        }
        Step::Rework(task) => state
            .rework_task(LEAD, &pick(ids, *task), "redo", NOW)
            .await
            .map(drop),
        Step::Fail(task) => state
            .fail_task(LEAD, &pick(ids, *task), "no")
            .await
            .map(drop),
        Step::Reopen(task) => state.reopen_task(LEAD, &pick(ids, *task)).await.map(drop),
        // The board answers in plain text; a rule of the table broken under it would
        // show as the trigger's or the index's words.
        Step::BoardComplete(task) => board(state.complete(LEAD, &pick(ids, *task)).await),
        Step::BoardAddItems(task) => {
            let items = ["a step".to_owned()];
            board(state.add_task_items(LEAD, &pick(ids, *task), &items).await)
        }
    };
    match made {
        Err(PlanError::Store(error)) => Err(format!("{step:?}: {error:#}")),
        _ => Ok(()),
    }
}

fn board<T>(made: anyhow::Result<T>) -> Result<(), PlanError> {
    match made {
        Err(error) => {
            let text = format!("{error:#}");
            let broken = [
                "disagree",
                "has no owner",
                "a lease",
                "constraint",
                "UNIQUE",
            ];
            if broken.iter().any(|word| text.contains(word)) {
                return Err(PlanError::Store(error));
            }
            Ok(())
        }
        Ok(_) => Ok(()),
    }
}

/// The rules. `team_ends` is how many times work was returned: each may leave a task
/// one attempt over the cap, since it goes back to open whatever its attempts.
async fn check(state: &State, team_ends: u32) -> Result<(), String> {
    use PlanStatus::{Blocked, Claimed, Done, Open, Review, Running};
    let rows = sqlx::query("SELECT task_id, done, status FROM tasks")
        .fetch_all(&state.pool)
        .await
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (id, done, status): (String, i64, String) =
            (row.get("task_id"), row.get("done"), row.get("status"));
        if (done == 1) != (status == "done") {
            return Err(format!("{id}: done {done}, status {status}"));
        }
    }
    let plan = state.plan(LEAD).await.map_err(|error| error.to_string())?;
    let by_id: HashMap<&str, &PlanTask> = plan
        .iter()
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let mut at_work: HashSet<&str> = HashSet::new();
    for task in &plan {
        let id = &task.task_id;
        let works = matches!(task.status, Claimed | Running);
        if works != task.lease_until.is_some() {
            return Err(format!(
                "{id}: {:?} with lease {:?}",
                task.status, task.lease_until
            ));
        }
        if works && task.owner_thread_id.is_none() {
            return Err(format!("{id}: at work without an owner"));
        }
        if task.status == Open && task.owner_thread_id.is_some() {
            return Err(format!("{id}: open with an owner"));
        }
        if task.reworks > MAX_REWORKS || task.attempts > MAX_ATTEMPTS + team_ends {
            return Err(format!(
                "{id}: {} attempts, {} reworks",
                task.attempts, task.reworks
            ));
        }
        if let (true, Some(owner)) = (works, task.owner_thread_id.as_deref())
            && !at_work.insert(owner)
        {
            return Err(format!("{owner} works on two tasks"));
        }
        // A task that started stands on done ground, and done is final.
        if matches!(task.status, Claimed | Running | Review) || task.owner_thread_id.is_some() {
            for dependency in &task.depends_on {
                let ground = by_id.get(dependency.as_str()).map(|task| task.status);
                if ground != Some(Done) && task.status != PlanStatus::Failed {
                    return Err(format!("{id} started on {dependency}, which is {ground:?}"));
                }
            }
        }
        // No way round: following dependencies from a task never comes back to it.
        let mut seen: HashSet<&str> = HashSet::new();
        let mut next: Vec<&str> = task.depends_on.iter().map(String::as_str).collect();
        while let Some(at) = next.pop() {
            if at == id {
                return Err(format!("{id} depends on itself"));
            }
            if seen.insert(at)
                && let Some(task) = by_id.get(at)
            {
                next.extend(task.depends_on.iter().map(String::as_str));
            }
        }
    }
    let holds = |task: &&PlanTask| {
        matches!(task.status, Claimed | Running | Review)
            || (task.status == Blocked && task.owner_thread_id.is_some())
    };
    let holders: Vec<&PlanTask> = plan.iter().filter(holds).collect();
    for (at, one) in holders.iter().enumerate() {
        for other in &holders[at + 1..] {
            if let Some((mine, theirs)) = one.files.overlap(&other.files) {
                assert!(overlaps(mine, theirs));
                return Err(format!(
                    "{} and {} both hold {mine} / {theirs}",
                    one.task_id, other.task_id
                ));
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn no_sequence_of_moves_breaks_the_plan(steps in prop::collection::vec(step(), 1..60)) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome: Result<(), String> = runtime.block_on(async {
            let state = team().await;
            let mut ids = Vec::new();
            let mut team_ends = 0;
            for step in &steps {
                make(&state, &mut ids, step).await?;
                if matches!(step, Step::ReturnWork) {
                    team_ends += 1;
                }
                check(&state, team_ends).await.map_err(|broken| format!("after {step:?}: {broken}"))?;
            }
            Ok(())
        });
        prop_assert_eq!(outcome, Ok(()));
    }
}
