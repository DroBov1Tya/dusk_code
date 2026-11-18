use crate::testing::temp_state;

mod claims;
mod graph;
mod kit;
mod machine;
mod migration;
mod property;

#[tokio::test]
async fn an_agent_reaches_only_its_own_tasks() {
    let state = temp_state("tasks_own").await;
    state.create_thread("mine", "test").await.unwrap();
    state.create_thread("other", "test").await.unwrap();
    let steps = vec!["read".to_owned(), "write".to_owned()];
    let task = state.add_task("mine", "port it", &steps).await.unwrap();
    let first = task.items[0].item_id.clone();

    assert!(
        state.complete("other", &first).await.is_err(),
        "another thread's step"
    );
    assert!(
        state.complete("other", &task.task_id).await.is_err(),
        "another thread's task"
    );

    let task = state
        .complete("mine", &first[first.len() - 8..])
        .await
        .unwrap();
    assert!(task.items[0].done && !task.items[1].done && !task.done);
    let task = state.complete("mine", &task.task_id).await.unwrap();
    assert!(task.done && task.items.iter().all(|item| item.done));
}

#[tokio::test]
async fn an_open_task_is_not_added_twice() {
    let state = temp_state("tasks_twice").await;
    state.create_thread("mine", "test").await.unwrap();
    let task = state.add_task("mine", "Port it", &[]).await.unwrap();
    let again = state.add_task("mine", " port it ", &[]).await.unwrap_err();
    assert!(again.to_string().contains("already open"), "{again}");
    state.complete("mine", &task.task_id).await.unwrap();
    state.add_task("mine", "port it", &[]).await.unwrap();
}

#[tokio::test]
async fn steps_run_one_after_another() {
    let state = temp_state("tasks_timers").await;
    state.create_thread("mine", "test").await.unwrap();
    let steps = vec!["read".to_owned(), "write".to_owned()];
    let task = state.add_task("mine", "port it", &steps).await.unwrap();
    let (read, write) = (&task.items[0], &task.items[1]);
    assert!(read.started_at.is_some() && write.started_at.is_none());

    let task = state.complete("mine", &read.item_id).await.unwrap();
    let (read, write) = (&task.items[0], &task.items[1]);
    assert!(read.finished_at.is_some() && write.started_at.is_some());

    let task = state.complete("mine", &write.item_id).await.unwrap();
    assert!(task.done, "the last step closes the task");
    let more = vec!["test".to_owned()];
    let task = state
        .add_task_items("mine", &task.task_id, &more)
        .await
        .unwrap();
    assert!(!task.done && task.items[2].started_at.is_some());
    assert_eq!(task.items[2].description, "test");
}
