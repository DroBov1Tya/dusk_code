//! The graph: dependencies, cycles, what is ready, the blocks behind a failed task, the
//! caps, whose plan a thread reaches, and input that means harm.

use super::kit::{LEAD, NOW, add, claim, drive, get, keyed, status, task, team, with_files};
use crate::state::{
    BlockedOn, MAX_DEPENDENCIES, MAX_FILES, MAX_OPEN_TASKS, MAX_TASKS_AN_ADD, NewTask, PassedBy,
    PlanError, PlanStatus, State, TaskEdit, TaskFiles, WaitsOn, overlaps,
};

fn after(ids: &[&str]) -> TaskEdit {
    TaskEdit {
        after: Some(ids.iter().map(|id| (*id).to_owned()).collect()),
        ..TaskEdit::default()
    }
}

async fn ready(state: &State) -> Vec<String> {
    let tasks = state.ready_tasks(LEAD).await.unwrap();
    tasks.into_iter().map(|task| task.task_id).collect()
}

async fn done(state: &State, id: &str, member: &str) {
    drive(state, id, member, PlanStatus::Done).await;
}

#[tokio::test]
async fn an_add_links_its_tasks_by_key_and_to_the_plan_by_id() {
    let state = team().await;
    let first = add(&state, &[task("the ground")]).await.remove(0);
    let end = &first[first.len() - 8..];
    let ids = add(
        &state,
        &[
            keyed("b", &["a", end]),
            keyed("a", &[]),
            // Named twice, kept once.
            keyed("c", &["a", "a", &format!("#{end}")]),
        ],
    )
    .await;
    assert_eq!(
        get(&state, &ids[0]).await.depends_on,
        [ids[1].clone(), first.clone()]
    );
    assert_eq!(
        get(&state, &ids[2]).await.depends_on,
        [ids[1].clone(), first.clone()]
    );

    let unknown = state
        .add_plan_tasks(LEAD, &[keyed("d", &["nowhere"])])
        .await
        .unwrap_err();
    assert!(
        matches!(unknown, PlanError::UnknownDependency(_)),
        "{unknown}"
    );
    let twice = state
        .add_plan_tasks(LEAD, &[keyed("d", &[]), keyed("d", &[])])
        .await
        .unwrap_err();
    assert!(matches!(twice, PlanError::DuplicateKey(_)), "{twice}");
    assert_eq!(
        state.plan(LEAD).await.unwrap().len(),
        4,
        "a refused add adds nothing"
    );
}

#[tokio::test]
async fn a_cycle_is_refused_and_nothing_is_written() {
    let state = team().await;
    let itself = state
        .add_plan_tasks(LEAD, &[keyed("a", &["a"])])
        .await
        .unwrap_err();
    assert!(
        matches!(&itself, PlanError::Cycle(path) if path.len() == 2),
        "{itself}"
    );
    assert!(
        itself.to_string().ends_with("depends on itself"),
        "{itself}"
    );

    let two = state
        .add_plan_tasks(LEAD, &[keyed("a", &["b"]), keyed("b", &["a"])])
        .await
        .unwrap_err();
    let PlanError::Cycle(path) = &two else {
        panic!("{two}");
    };
    assert_eq!(path.len(), 3);
    let (a, b) = (&path[0][path[0].len() - 8..], &path[1][path[1].len() - 8..]);
    assert_eq!(
        two.to_string(),
        format!("task #{a} depends on #{b}, which depends on #{a}")
    );

    let long: Vec<NewTask> = (0..20)
        .map(|at| keyed(&format!("k{at}"), &[&format!("k{}", (at + 1) % 20)]))
        .collect();
    let long = state.add_plan_tasks(LEAD, &long).await.unwrap_err();
    assert!(
        matches!(&long, PlanError::Cycle(path) if path.len() == 21),
        "{long}"
    );
    assert!(state.plan(LEAD).await.unwrap().is_empty());

    // Through an edit: a chain a <- b <- c, then a is made to depend on c.
    let ids = add(
        &state,
        &[keyed("a", &[]), keyed("b", &["a"]), keyed("c", &["b"])],
    )
    .await;
    let before = state.plan(LEAD).await.unwrap();
    let closed = state
        .edit_plan_task(LEAD, &ids[0], &after(&[&ids[2]]))
        .await
        .unwrap_err();
    assert!(
        matches!(&closed, PlanError::Cycle(path) if path.len() == 4),
        "{closed}"
    );
    let own = state
        .edit_plan_task(LEAD, &ids[0], &after(&[&ids[0]]))
        .await
        .unwrap_err();
    assert!(matches!(own, PlanError::Cycle(_)), "{own}");
    assert_eq!(state.plan(LEAD).await.unwrap(), before);
    // A diamond is no cycle.
    add(&state, &[keyed("d", &[&ids[1], &ids[2], &ids[0]])]).await;
}

#[tokio::test]
async fn ready_is_open_with_every_dependency_done() {
    let state = team().await;
    let ids = add(
        &state,
        &[
            NewTask {
                phase: "2".to_owned(),
                ..keyed("late", &[])
            },
            NewTask {
                phase: "1".to_owned(),
                ..keyed("a", &[])
            },
            NewTask {
                phase: "1".to_owned(),
                ..keyed("b", &["a"])
            },
            NewTask {
                phase: "1".to_owned(),
                ..keyed("c", &["a", "b"])
            },
        ],
    )
    .await;
    // By phase label, then oldest first.
    assert_eq!(ready(&state).await, [ids[1].clone(), ids[0].clone()]);
    // A dependency at work, in review or failed is not done.
    drive(&state, &ids[1], "m0", PlanStatus::Review).await;
    assert_eq!(ready(&state).await, [ids[0].clone()]);
    let early = state
        .claim_task(LEAD, &ids[2], claim("m1"))
        .await
        .unwrap_err();
    assert!(
        matches!(&early, PlanError::NotClaimable { why, .. }
            if *why == crate::state::Unclaimable::Dependency(ids[1].clone())),
        "{early}"
    );
    state
        .pass_task(LEAD, &ids[1], PassedBy::Lead)
        .await
        .unwrap();
    assert_eq!(ready(&state).await, [ids[2].clone(), ids[0].clone()]);
    done(&state, &ids[2], "m1").await;
    assert_eq!(ready(&state).await, [ids[3].clone(), ids[0].clone()]);
    // A dependency whose row is gone is not done either.
    sqlx::query("UPDATE tasks SET depends_on = json_array('gone') WHERE task_id = ?")
        .bind(&ids[3])
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(ready(&state).await, [ids[0].clone()]);
    assert!(state.claim_task(LEAD, &ids[3], claim("m2")).await.is_err());
}

#[tokio::test]
async fn a_failed_task_blocks_what_stands_behind_it_until_it_is_reopened() {
    let state = team().await;
    // a <- b <- c, and d <- c too; e depends on nothing.
    let ids = add(
        &state,
        &[
            keyed("a", &[]),
            keyed("b", &["a"]),
            keyed("d", &[]),
            keyed("c", &["b", "d"]),
            keyed("e", &[]),
        ],
    )
    .await;
    let (a, b, d, c, e) = (&ids[0], &ids[1], &ids[2], &ids[3], &ids[4]);
    state.fail_task(LEAD, a, "no").await.unwrap();
    for behind in [b, c] {
        let task = get(&state, behind).await;
        assert_eq!(task.status, PlanStatus::Blocked);
        assert_eq!(task.waits_on, Some(WaitsOn::Task(a.clone())));
        assert_eq!(task.owner_thread_id, None);
    }
    assert_eq!(status(&state, e).await, PlanStatus::Open);
    assert_eq!(status(&state, d).await, PlanStatus::Open);

    // A second failed task before c: reopening the first leaves c blocked on the other.
    state.fail_task(LEAD, d, "no").await.unwrap();
    state.reopen_task(LEAD, a).await.unwrap();
    assert_eq!(status(&state, b).await, PlanStatus::Open);
    let still = get(&state, c).await;
    assert_eq!(still.status, PlanStatus::Blocked);
    assert_eq!(still.waits_on, Some(WaitsOn::Task(d.clone())));
    state.reopen_task(LEAD, d).await.unwrap();
    let open = get(&state, c).await;
    assert_eq!((open.status, open.waits_on), (PlanStatus::Open, None));

    // A release at the third attempt fails the task, and that blocks as `fail` does.
    for _ in 0..3 {
        state.claim_task(LEAD, a, claim("m0")).await.unwrap();
        state.release_task(LEAD, a, "stopped").await.unwrap();
    }
    assert_eq!(status(&state, a).await, PlanStatus::Failed);
    assert_eq!(status(&state, c).await, PlanStatus::Blocked);
    // A task added behind a failed one is blocked from the start; an edit that takes
    // the dependency away opens it.
    let new = add(&state, &[keyed("f", &[a])]).await.remove(0);
    assert_eq!(status(&state, &new).await, PlanStatus::Blocked);
    let freed = state.edit_plan_task(LEAD, &new, &after(&[])).await.unwrap();
    assert_eq!((freed.status, freed.waits_on), (PlanStatus::Open, None));
    // A member's blocked task is not the graph's to open.
    drive(&state, e, "m1", PlanStatus::Running).await;
    state
        .block_task("m1", e, Some(&BlockedOn::Task(a.clone())), "needs a")
        .await
        .unwrap();
    state.reopen_task(LEAD, a).await.unwrap();
    assert_eq!(status(&state, e).await, PlanStatus::Blocked);
    assert_eq!(status(&state, c).await, PlanStatus::Open);
}

#[tokio::test]
async fn an_edit_is_for_a_task_nobody_started() {
    let state = team().await;
    let ids = add(&state, &[task("one"), task("two")]).await;
    let edit = TaskEdit {
        description: Some(" other words ".to_owned()),
        role: Some(Some("reviewer".to_owned())),
        phase: Some("2".to_owned()),
        acceptance: Some("it passes".to_owned()),
        brief: Some("mind the gap".to_owned()),
        gate: Some(Some(dusk_protocol::thread::GoalCheck::UserConfirmation)),
        files: Some(TaskFiles::new(["src/"]).unwrap()),
        after: Some(vec![ids[1].clone()]),
        tier: Some(dusk_protocol::config::ModelTier::Strong),
        effort: Some(dusk_protocol::model::Effort::High),
    };
    let edited = state.edit_plan_task(LEAD, &ids[0], &edit).await.unwrap();
    assert_eq!(edited.description, "other words");
    assert_eq!(edited.tier, Some(dusk_protocol::config::ModelTier::Strong));
    assert_eq!(edited.effort, Some(dusk_protocol::model::Effort::High));
    assert_eq!(edited.role.as_deref(), Some("reviewer"));
    assert_eq!(edited.files.entries(), ["src/"]);
    assert_eq!(edited.depends_on, [ids[1].clone()]);
    assert_eq!(
        state
            .edit_plan_task(LEAD, &ids[0], &TaskEdit::default())
            .await
            .unwrap()
            .phase,
        "2"
    );

    for to in [
        PlanStatus::Claimed,
        PlanStatus::Running,
        PlanStatus::Blocked,
        PlanStatus::Review,
        PlanStatus::Done,
    ] {
        let id = add(&state, &[task(to.as_str())]).await.remove(0);
        let member = format!("m{}", to as usize);
        drive(&state, &id, &member, to).await;
        let refused = state.edit_plan_task(LEAD, &id, &edit).await.unwrap_err();
        assert!(
            matches!(refused, PlanError::Illegal { from, .. } if from == to),
            "{refused}"
        );
    }
}

#[tokio::test]
async fn the_caps_hold() {
    let state = team().await;
    let many: Vec<NewTask> = (0..=MAX_TASKS_AN_ADD)
        .map(|at| task(&format!("t{at}")))
        .collect();
    let refused = state.add_plan_tasks(LEAD, &many).await.unwrap_err();
    assert!(matches!(refused, PlanError::TooMany { max, .. } if max == MAX_TASKS_AN_ADD));
    assert!(matches!(
        state.add_plan_tasks(LEAD, &[]).await.unwrap_err(),
        PlanError::Empty(_)
    ));
    let ids = add(&state, &many[..MAX_TASKS_AN_ADD]).await;
    add(&state, &many[..MAX_TASKS_AN_ADD]).await;
    let full = state
        .add_plan_tasks(LEAD, &[task("one more")])
        .await
        .unwrap_err();
    assert!(
        matches!(full, PlanError::TooMany { max, .. } if max == MAX_OPEN_TASKS),
        "{full}"
    );
    // Done tasks do not count; failed ones do.
    state.fail_task(LEAD, &ids[1], "no").await.unwrap();
    assert!(
        state
            .add_plan_tasks(LEAD, &[task("one more")])
            .await
            .is_err()
    );
    done(&state, &ids[0], "m0").await;
    add(&state, &[task("one more")]).await;

    let files: Vec<String> = (0..=MAX_FILES).map(|at| format!("src/f{at}.rs")).collect();
    assert!(matches!(
        TaskFiles::new(&files).unwrap_err(),
        PlanError::TooMany { max, .. } if max == MAX_FILES
    ));
    assert_eq!(
        TaskFiles::new(&files[..MAX_FILES]).unwrap().entries().len(),
        MAX_FILES
    );
    // The same entry twice is one entry.
    assert_eq!(TaskFiles::new(["a", "a"]).unwrap().entries(), ["a"]);
}

#[tokio::test]
async fn a_thread_reaches_the_plan_of_its_own_chat_only() {
    let state = team().await;
    let ids = add(&state, &[with_files("ours", &["src/"]), task("second")]).await;
    let ours = &ids[0];
    // A member reads and the lead's plan is one plan; the member's own board is empty.
    assert_eq!(state.plan("m3").await.unwrap().len(), 2);
    assert_eq!(state.plan_task("m3", ours).await.unwrap().task_id, *ours);
    assert!(state.thread_tasks("m3").await.unwrap().is_empty());
    assert_eq!(state.thread_tasks(LEAD).await.unwrap().len(), 2);

    drive(&state, &ids[1], "m1", PlanStatus::Review).await;
    let review = &ids[1];
    let before = state.plan(LEAD).await.unwrap();
    for thread in ["other", "other-m"] {
        assert!(state.plan(thread).await.unwrap().is_empty());
        assert!(state.ready_tasks(thread).await.unwrap().is_empty());
        let edit = TaskEdit {
            description: Some("theirs".to_owned()),
            ..TaskEdit::default()
        };
        let refusals = [
            state.plan_task(thread, ours).await.map(drop),
            state.edit_plan_task(thread, ours, &edit).await.map(drop),
            state
                .claim_task(thread, ours, claim("other-m"))
                .await
                .map(drop),
            state.fail_task(thread, ours, "no").await.map(drop),
            state.reopen_task(thread, ours).await.map(drop),
            state.release_task(thread, ours, "").await.map(drop),
            state.unblock_task(thread, ours, NOW).await.map(drop),
            state
                .pass_task(thread, review, PassedBy::Lead)
                .await
                .map(drop),
            state
                .rework_task(thread, review, "redo", NOW)
                .await
                .map(drop),
            state.hold_for_lead(thread, review, "x").await.map(drop),
            state.set_gate_job(thread, review, "job").await.map(drop),
            state.hand_in(thread, ours).await.map(drop),
            state.block_task(thread, ours, None, "x").await.map(drop),
            state.start_task(thread, ours, "turn").await.map(drop),
            state.renew_lease(thread, ours, NOW).await,
            state.renew_lease(thread, review, NOW).await,
        ];
        for (at, refusal) in refusals.into_iter().enumerate() {
            let error = refusal.unwrap_err();
            assert!(
                matches!(error, PlanError::NoSuchTask(_)),
                "{thread} {at}: {error}"
            );
        }
        assert!(
            state
                .release_expired(thread, i64::MAX)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(state.return_work(thread).await.unwrap().is_empty());
        // Their plan cannot lean on ours.
        let lean = state
            .add_plan_tasks(thread, &[keyed("x", &[ours])])
            .await
            .unwrap_err();
        assert!(matches!(lean, PlanError::UnknownDependency(_)), "{lean}");
    }
    assert_eq!(state.plan(LEAD).await.unwrap(), before);

    // Our plan is not given to a thread of another chat, and no thread is no plan.
    let foreign = state
        .claim_task(LEAD, ours, claim("other-m"))
        .await
        .unwrap_err();
    assert!(
        matches!(&foreign, PlanError::NotClaimable { why, .. }
            if *why == crate::state::Unclaimable::ForeignOwner),
        "{foreign}"
    );
    let nobody = state.plan("nobody").await.unwrap_err();
    assert!(matches!(nobody, PlanError::NoSuchThread(_)), "{nobody}");
    // Their own task with the same files is theirs to give out.
    let theirs = state
        .add_plan_tasks("other", &[with_files("theirs", &["src/"])])
        .await
        .unwrap()
        .remove(0);
    state.claim_task(LEAD, ours, claim("m0")).await.unwrap();
    state
        .claim_task("other-m", &theirs.task_id, claim("other-m"))
        .await
        .unwrap();
}

#[test]
fn files_have_one_shape() {
    for good in [
        "a",
        "src/a.rs",
        "src/",
        "a b/c d.txt",
        "src/[id].tsx",
        "a%b/_c'\"d",
        ".env",
        "a..b",
    ] {
        assert!(TaskFiles::new([good]).is_ok(), "{good}");
    }
    let long = "a/".repeat(300);
    for bad in [
        "",
        "/",
        "/etc/passwd",
        "..",
        "../a",
        "a/../b",
        "a/..",
        "./a",
        "a/./b",
        "a//b",
        "a//",
        "//a",
        "a\\b",
        "C:/x",
        "c:",
        "src/*.rs",
        "src/?",
        "a\nb",
        "a\0b",
        "a\tb",
        &long,
    ] {
        let refused = TaskFiles::new([bad]).unwrap_err();
        assert!(
            matches!(refused, PlanError::BadFile { .. }),
            "{bad:?}: {refused}"
        );
    }
}

#[test]
fn entries_overlap_when_equal_or_one_is_a_folder_above() {
    let cases = [
        ("src/a.rs", "src/a.rs", true),
        ("src/a.rs", "src/b.rs", false),
        ("src/", "src/a.rs", true),
        ("src/a.rs", "src/", true),
        ("src/", "src/deep/er/x", true),
        ("src/deep/", "src/", true),
        ("src/a/", "src/b/", false),
        // A prefix that is no folder.
        ("src/a", "src/ab", false),
        ("src/a/", "src/ab", false),
        ("src/a/", "src/ab/", false),
        // One path, written as a file and as a folder.
        ("src/a", "src/a/", true),
        ("src/a", "src/a/x", false),
        // Nothing is a pattern.
        ("src/%/", "src/x/y", false),
        ("src/a_c", "src/abc", false),
        ("%", "a", false),
    ];
    for (one, other, expected) in cases {
        assert_eq!(overlaps(one, other), expected, "{one} {other}");
        assert_eq!(overlaps(other, one), expected, "{other} {one}");
    }
}

#[tokio::test]
async fn hostile_text_is_stored_as_text_or_refused() {
    let state = team().await;
    let texts = [
        "'; DROP TABLE tasks; --",
        "\"quoted\" 'and' `ticked`",
        "100%_done\\",
        "line one\nline two\r\n\ttab",
        "$1 ?1 :name @x",
        "\u{202e}reversed \u{0000} nul",
    ];
    for text in texts {
        let new = NewTask {
            key: Some(text.to_owned()),
            description: text.to_owned(),
            role: Some(text.to_owned()),
            phase: text.chars().take(32).collect(),
            acceptance: text.to_owned(),
            brief: text.to_owned(),
            ..NewTask::default()
        };
        let id = add(&state, &[new]).await.remove(0);
        let stored = get(&state, &id).await;
        assert_eq!(stored.description, text.trim());
        assert_eq!(stored.role.as_deref(), Some(text));
        assert_eq!(stored.brief, text);
        state.fail_task(LEAD, &id, text).await.unwrap();
        assert_eq!(get(&state, &id).await.note, text);
        // As a dependency or an id it names no task; `%` and `_` match nothing.
        let unknown = state
            .add_plan_tasks(LEAD, &[keyed("k", &[text])])
            .await
            .unwrap_err();
        assert!(
            matches!(unknown, PlanError::UnknownDependency(_)),
            "{unknown}"
        );
    }
    for pattern in [
        "%%%%",
        "____",
        "%",
        "________-____-____-____-____________",
        "",
        "   ",
        "#",
    ] {
        let found = state.plan_task(LEAD, pattern).await.unwrap_err();
        assert!(
            matches!(found, PlanError::NoSuchTask(_)),
            "{pattern}: {found}"
        );
        assert!(state.fail_task(LEAD, pattern, "x").await.is_err());
    }

    let huge = "x".repeat(10_000);
    let too_long = |new: NewTask| {
        let state = &state;
        async move {
            let refused = state.add_plan_tasks(LEAD, &[new]).await.unwrap_err();
            assert!(matches!(refused, PlanError::TooLong { .. }), "{refused}");
        }
    };
    too_long(task(&huge)).await;
    too_long(NewTask {
        key: Some(huge.clone()),
        ..task("t")
    })
    .await;
    too_long(NewTask {
        role: Some(huge.clone()),
        ..task("t")
    })
    .await;
    too_long(NewTask {
        phase: huge.clone(),
        ..task("t")
    })
    .await;
    too_long(NewTask {
        acceptance: huge.clone(),
        ..task("t")
    })
    .await;
    too_long(NewTask {
        brief: huge.clone(),
        ..task("t")
    })
    .await;
    assert!(matches!(
        state
            .add_plan_tasks(LEAD, &[task(" \n ")])
            .await
            .unwrap_err(),
        PlanError::Empty(_)
    ));
    // Counted in characters, as the table's CHECK counts: 600 two-byte letters fit.
    let id = add(
        &state,
        &[NewTask {
            acceptance: "é".repeat(600),
            ..task("wide")
        }],
    )
    .await
    .remove(0);
    let wide = state
        .fail_task(LEAD, &id, &"é".repeat(601))
        .await
        .unwrap_err();
    assert!(matches!(wide, PlanError::TooLong { .. }), "{wide}");
    assert!(matches!(
        state.plan_task(LEAD, &huge).await.unwrap_err(),
        PlanError::NoSuchTask(_)
    ));
    assert!(matches!(
        state
            .claim_task(
                LEAD,
                &id,
                crate::state::Claim {
                    model: Some(&huge),
                    ..claim("m0")
                }
            )
            .await
            .unwrap_err(),
        PlanError::TooLong { .. }
    ));
    assert!(matches!(
        TaskFiles::new([&huge]).unwrap_err(),
        PlanError::BadFile { .. }
    ));

    let thousand: Vec<String> = (0..1_000).map(|at| format!("dep-{at}")).collect();
    let refused = state
        .add_plan_tasks(
            LEAD,
            &[NewTask {
                after: thousand.clone(),
                ..task("t")
            }],
        )
        .await
        .unwrap_err();
    assert!(matches!(refused, PlanError::TooMany { max, .. } if max == MAX_DEPENDENCIES));
    let refused = state
        .edit_plan_task(
            LEAD,
            &id,
            &TaskEdit {
                after: Some(thousand),
                ..TaskEdit::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(refused, PlanError::TooMany { .. }), "{refused}");
    // The table is whole and the errors show a bounded part of what they were given.
    assert!(
        sqlx::query("SELECT COUNT(*) FROM tasks")
            .fetch_one(&state.pool)
            .await
            .is_ok()
    );
    assert!(PlanError::NoSuchTask(huge).to_string().len() < 200);
}

#[tokio::test]
async fn files_with_pattern_letters_are_compared_as_text_in_the_claim() {
    let state = team().await;
    let ids = add(
        &state,
        &[
            with_files("percent", &["src/%/"]),
            with_files("plain", &["src/x/y.rs"]),
            with_files("under", &["src/a_c"]),
            with_files("letter", &["src/abc"]),
            with_files("quote", &["it's/\"x\"/"]),
            with_files("inside", &["it's/\"x\"/y"]),
        ],
    )
    .await;
    for (at, id) in ids.iter().take(5).enumerate() {
        state
            .claim_task(LEAD, id, claim(&format!("m{at}")))
            .await
            .unwrap();
    }
    let held = state
        .claim_task(LEAD, &ids[5], claim("m5"))
        .await
        .unwrap_err();
    assert!(matches!(held, PlanError::NotClaimable { .. }), "{held}");
}
