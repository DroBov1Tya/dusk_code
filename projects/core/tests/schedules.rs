//! Scheduled tasks against a real core: a task is kept only for a saved project, and a
//! run opens a new chat that the timetable remembers.

use dusk_core::app::{Paths, start_hub};
use dusk_protocol::{
    client::{Hello, PROTOCOL_VERSION},
    project::{Clock, Repeat, ScheduleDraft},
};

fn draft(project_id: &str) -> ScheduleDraft {
    ScheduleDraft {
        id: None,
        project_id: project_id.to_owned(),
        prompt: "say hello".to_owned(),
        repeat: Repeat::Daily {
            at: Clock { hour: 9, minute: 0 },
        },
        enabled: true,
        model: None,
    }
}

#[tokio::test]
async fn a_task_runs_only_in_a_saved_project_and_remembers_its_chat() {
    let root = std::env::temp_dir().join(format!("dusk-schedules-{}", std::process::id()));
    let folder = root.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    let client = hub
        .attach(Hello {
            protocol: PROTOCOL_VERSION,
            client: "test".to_owned(),
        })
        .unwrap();
    let projects = client.projects();

    // Deny by default: no saved project, no task.
    assert!(projects.save_schedule(draft("nowhere")).await.is_err());

    let project = projects
        .save(None, "demo".into(), String::new(), folder)
        .await
        .unwrap();
    let saved = projects.save_schedule(draft(&project.id)).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert!(
        saved[0].next_run_at.is_some(),
        "a daily task has a next run"
    );

    let thread_id = projects.run_schedule(saved[0].id.clone()).await.unwrap();
    let after = projects.schedules().await.unwrap();
    assert_eq!(after[0].last_thread_id.as_deref(), Some(thread_id.as_str()));
    assert_eq!(
        after[0].next_run_at, saved[0].next_run_at,
        "run now keeps the timetable"
    );

    projects.delete_schedule(saved[0].id.clone()).await.unwrap();
    assert!(projects.schedules().await.unwrap().is_empty());
    client.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}
