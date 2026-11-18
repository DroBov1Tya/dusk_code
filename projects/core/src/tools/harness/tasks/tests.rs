use serde_json::json;

use super::*;

#[test]
fn steps_may_be_text_or_objects() {
    let add = json!({ "action": "add", "task": "port", "items": ["read", { "text": "write" }] });
    let add: TasksArguments = parse_arguments(add).unwrap();
    let items: Vec<String> = add.items.into_iter().map(Step::into_text).collect();
    assert_eq!(items, ["read", "write"]);
    let schema = schema::of::<TasksArguments>();
    let check = schema::Check::new(&schema);
    let objects = json!({ "action": "add", "task": "port", "items": [{ "text": "read" }] });
    assert!(check.problems("tasks", &schema, &objects).is_none());
    let wrong = json!({ "action": "drop", "id": "a1b2c3d4", "force": true });
    assert!(check.problems("tasks", &schema, &wrong).is_some());
}
