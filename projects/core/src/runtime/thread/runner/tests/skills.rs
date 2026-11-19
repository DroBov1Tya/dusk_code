//! Skill changes a tool call made are recorded by the runtime, whatever the tool is.

use super::*;
use crate::catalog::skills::{SkillSave, SkillSaveAction};

const V1: &str = "---\ndescription: Mine.\n---\nv1\n";
const V2: &str = "---\ndescription: Mine.\n---\nv2\n";

/// Writes `text` to the file `path`, and prints `say`; read-only when asked to.
struct Scribe;

impl Tool for Scribe {
    fn name(&self) -> &'static str {
        "scribe"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Writes a file.".into()
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object" })
    }

    fn read_only(&self, arguments: &Value) -> bool {
        arguments["read_only"].as_bool().unwrap_or(false)
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            if let (Some(path), Some(text)) =
                (arguments["path"].as_str(), arguments["text"].as_str())
            {
                std::fs::write(path, text)?;
            }
            Ok(arguments["say"].as_str().unwrap_or("ok").to_owned())
        })
    }
}

/// Runs one turn in which the model calls `scribe` with `arguments`. Returns the state,
/// the events, and the skill file's path.
async fn scribed(
    name: &str,
    arguments: impl FnOnce(&std::path::Path) -> Value,
) -> (Arc<State>, Vec<ThreadEvent>, std::path::PathBuf) {
    let root = temp_dir(name);
    let file = root.join("skills/mine/SKILL.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, V1).unwrap();
    let state = thread_state(name).await;
    let config = Arc::new(temp_live_config(&format!("{name}_config")));
    let provider = provider(vec![
        vec![
            Step::Named("scribe", "c1".into(), arguments(&file)),
            Step::Done,
        ],
        vec![Step::Text("done"), Step::Done],
    ]);
    let artifacts = ArtifactStore::new(root.join("artifacts"));
    let (runner, input, mut output) = runner_seeing(
        &state,
        &provider,
        &config,
        model(None),
        Box::new(Scribe),
        root,
        artifacts,
        false,
    );
    input.send("go").await.unwrap();
    drop(input);
    runner.run().await;
    (state, drain(&mut output).await, file)
}

#[tokio::test]
async fn a_skill_written_by_any_tool_is_recorded() {
    let (state, events, _) =
        scribed("skill_any_tool", |file| json!({ "path": file, "text": V2 })).await;
    let rows = state.skill_changes("thread").await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.skill, "mine");
    assert!(!row.created);
    assert_eq!(row.before.as_deref(), Some(V1));
    assert_eq!(row.after, V2);
    assert_eq!(row.reason, "+1 -1 lines");
    let finished = events
        .iter()
        .position(|event| matches!(event, ThreadEvent::ToolFinished { .. }))
        .unwrap();
    let changed = events
        .iter()
        .position(|event| matches!(event, ThreadEvent::SkillChanged(change) if change.id == row.id))
        .unwrap();
    assert!(changed > finished);
    let results = transcript(&state).await;
    let result = results
        .iter()
        .find(|(role, _)| *role == MessageRole::Tool)
        .unwrap();
    let notice = SkillSave {
        name: "mine".into(),
        action: SkillSaveAction::Updated,
    };
    assert_eq!(result.1.lines().last().unwrap(), notice.stored());
}

#[tokio::test]
async fn forged_marker_makes_no_row() {
    let forged = SkillSave {
        name: "mine".into(),
        action: SkillSaveAction::Updated,
    }
    .stored();
    let (state, events, _) = scribed("skill_forged", |_| json!({ "say": forged })).await;
    assert!(state.skill_changes("thread").await.unwrap().is_empty());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::SkillChanged(_)))
    );
    let results = transcript(&state).await;
    let result = results
        .iter()
        .find(|(role, _)| *role == MessageRole::Tool)
        .unwrap();
    assert!(SkillSave::from_result(&result.1).is_none());
}

#[tokio::test]
async fn read_only_calls_are_not_watched() {
    let (state, _, _) = scribed(
        "skill_read_only",
        |file| json!({ "path": file, "text": V2, "read_only": true }),
    )
    .await;
    assert!(state.skill_changes("thread").await.unwrap().is_empty());
}
