//! `plan_edit`: the pinned plans of the chat, Markdown files among its sources. The
//! first call creates one; later calls rewrite it whole, or delete it once it no longer
//! describes the work. Pinned sources come back with every request, so a plan stays
//! between the user and the model; the one written last is the current plan.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::thread::PlanFile;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    BoxFuture,
    message::sources,
    state::{Source, State},
    tools::{Tool, ToolContext, schema, schema::parse_arguments},
};

/// The name a plan goes by when it is given none.
pub const PLAN_NAME: &str = "plan.md";
const MAX_PLAN_BYTES: usize = 256 * 1024;

pub struct PlanEdit {
    state: Arc<State>,
    thread_id: String,
}

/// Flat, since providers want one object at the top of a schema.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// The whole plan as Markdown, not a diff. Left out with `delete`.
    content: Option<String>,
    /// The name the plan goes by; defaults to plan.md, or with `delete` to the current plan.
    title: Option<String>,
    /// Deletes the plan instead of writing it.
    #[serde(default)]
    delete: bool,
}

impl PlanEdit {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}

impl Tool for PlanEdit {
    /// Keeps the chat's own records or speaks to someone: no project file changes.
    fn writes_project(&self, _arguments: &Value) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "plan_edit"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Create or rewrite this chat's pinned plan: one Markdown file, pinned so the whole \
         of it comes back with every message. Give the whole plan as Markdown, not a patch; \
         the plan keeps one name, so call the tool again to update it. With delete: true \
         it deletes a plan instead: remove a plan that is finished or was replaced and no \
         longer describes the work, rather than leave it pinned. The current plan goes only \
         once finished; until then rewrite it."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let workspace = context.scope.workspace();
            let title = arguments.title.as_deref().map(sources::clean_name);
            if arguments.delete {
                ensure!(arguments.content.is_none(), "delete takes no content");
                // Only plans of this chat; an attached file of the name is no plan.
                let plans = list(&self.state, workspace, &self.thread_id).await?;
                let plan = match &title {
                    Some(name) => plans.into_iter().find(|plan| &plan.name == name),
                    None => plans.into_iter().next(),
                };
                let Some(plan) = plan else {
                    match title {
                        Some(name) => bail!("this chat has no plan named {name}"),
                        None => bail!("this chat has no plan"),
                    }
                };
                // The plan the work goes by stays until it is done: the user may have
                // written in it, and deleting it would make an older one current.
                ensure!(
                    plan.old(),
                    "the current plan {} is not finished; rewrite it instead",
                    plan.name
                );
                remove_source(&self.state, workspace, &self.thread_id, &plan.source_id).await?;
                return Ok(format!("the plan {} is deleted", plan.name));
            }
            let content = arguments.content.unwrap_or_default();
            ensure!(!content.trim().is_empty(), "content is empty");
            let name = title.unwrap_or_else(|| PLAN_NAME.to_owned());
            let saved = save(&self.state, workspace, &self.thread_id, &name, &content).await?;
            Ok(match saved {
                Saved::Updated { bytes } => {
                    format!("the pinned plan {name} is updated ({bytes} bytes)")
                }
                Saved::Created { bytes, source_id } => format!(
                    "the pinned plan {name} is created ({bytes} bytes, source #{})",
                    &source_id[source_id.len() - 8..]
                ),
            })
        })
    }
}

pub enum Saved {
    Created { bytes: usize, source_id: String },
    Updated { bytes: usize },
}

/// Writes the plan named `name` among the chat's sources: the first time as a new
/// pinned source, later over the same file. Either way it becomes the current plan.
pub async fn save(
    state: &State,
    workspace: &Path,
    thread_id: &str,
    name: &str,
    content: &str,
) -> Result<Saved> {
    ensure!(content.len() <= MAX_PLAN_BYTES, "the plan is over 256 KiB");
    let bytes = content.as_bytes();
    let folder = workspace.join(sources::FOLDER);
    // Only a plan is written over: an attached file of the name stays as it is.
    let existing = state
        .plans(thread_id)
        .await?
        .into_iter()
        .map(|(source, _)| source)
        .find(|source| source.name == name);
    if let Some(source) = existing {
        write(&folder.join(&source.file), bytes).await?;
        state
            .set_source_bytes(thread_id, &source.source_id, bytes.len() as u64)
            .await?;
        state
            .mark_plan_written(thread_id, &source.source_id)
            .await?;
        return Ok(Saved::Updated { bytes: bytes.len() });
    }
    crate::init::create_private_dir(&folder)?;
    let source_id = Uuid::now_v7().to_string();
    // The end of the ID differs between sources added together.
    let tail = source_id[source_id.len() - 6..].to_owned();
    let source = Source {
        source_id,
        name: name.to_owned(),
        file: format!("{tail}-{name}"),
        bytes: bytes.len() as u64,
        pinned: false,
        delivered: false,
        plan: true,
    };
    write(&folder.join(&source.file), bytes).await?;
    state.add_source(thread_id, &source).await?;
    // `add_source` records new sources unpinned; the plan starts pinned.
    state.pin_source(thread_id, &source.source_id, true).await?;
    state
        .mark_plan_written(thread_id, &source.source_id)
        .await?;
    Ok(Saved::Created {
        bytes: bytes.len(),
        source_id: source.source_id,
    })
}

/// The chat's current plan, or none before one was written.
pub async fn read(state: &State, workspace: &Path, thread_id: &str) -> Result<Option<String>> {
    let Some(source) = current(state, thread_id).await? else {
        return Ok(None);
    };
    let path = workspace.join(sources::FOLDER).join(&source.file);
    let text = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("read the plan {}", path.display()))?;
    Ok(Some(text))
}

/// Drops the current plan, as the user's empty edit asks.
pub async fn clear(state: &State, workspace: &Path, thread_id: &str) -> Result<()> {
    if let Some(source) = current(state, thread_id).await? {
        remove_source(state, workspace, thread_id, &source.source_id).await?;
    }
    Ok(())
}

/// The name the user's edit is saved under: the current plan's, or the default.
pub async fn current_name(state: &State, thread_id: &str) -> Result<String> {
    Ok(current(state, thread_id)
        .await?
        .map_or_else(|| PLAN_NAME.to_owned(), |plan| plan.name))
}

/// The chat's plans, newest written first, each read to tell whether it is finished.
pub async fn list(state: &State, workspace: &Path, thread_id: &str) -> Result<Vec<PlanFile>> {
    let folder = workspace.join(sources::FOLDER);
    let mut plans = Vec::new();
    for (index, (source, written_at)) in state.plans(thread_id).await?.into_iter().enumerate() {
        // A file gone from the folder is a plan with nothing left to finish.
        let text = tokio::fs::read_to_string(folder.join(&source.file))
            .await
            .unwrap_or_default();
        plans.push(PlanFile {
            source_id: source.source_id,
            name: source.name,
            bytes: source.bytes,
            written_at,
            current: index == 0,
            finished: finished(&text),
        });
    }
    Ok(plans)
}

/// Deletes the plans a newer one replaced and the finished ones; says how many.
pub async fn remove_old(state: &State, workspace: &Path, thread_id: &str) -> Result<u64> {
    let mut removed = 0;
    for plan in list(state, workspace, thread_id).await? {
        if plan.old() {
            remove_source(state, workspace, thread_id, &plan.source_id).await?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// A plan with checkboxes, every one of them ticked.
fn finished(text: &str) -> bool {
    let (mut ticked, mut open) = (0, 0);
    for line in text.lines() {
        let item = line.trim_start();
        let item = item
            .strip_prefix(['-', '*', '+'])
            .or_else(|| {
                let digits = item.trim_start_matches(|c: char| c.is_ascii_digit());
                (digits.len() < item.len())
                    .then(|| digits.strip_prefix(['.', ')']))
                    .flatten()
            })
            .map(str::trim_start);
        match item.map(|item| item.get(..3)) {
            Some(Some("[ ]")) => open += 1,
            Some(Some("[x]" | "[X]")) => ticked += 1,
            _ => {}
        }
    }
    ticked > 0 && open == 0
}

async fn current(state: &State, thread_id: &str) -> Result<Option<Source>> {
    Ok(state
        .plans(thread_id)
        .await?
        .into_iter()
        .next()
        .map(|(source, _)| source))
}

async fn remove_source(state: &State, workspace: &Path, thread_id: &str, id: &str) -> Result<()> {
    sources::ThreadSources {
        state,
        workspace,
        thread_id,
    }
    .remove(id)
    .await
}

async fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    tokio::fs::write(path, bytes)
        .await
        .with_context(|| format!("write the plan {}", path.display()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        testing::{temp_dir, temp_state, tool_context},
        tools::ToolScope,
    };

    #[tokio::test]
    async fn the_first_call_creates_the_pinned_plan_and_the_next_rewrites_it() {
        let state = Arc::new(temp_state("plan_edit").await);
        state.create_thread("thread", "test").await.unwrap();
        let root = temp_dir("plan_edit_scope");
        std::fs::create_dir_all(&root).unwrap();
        let scope = ToolScope::new(&root, &root).unwrap();
        let tool = PlanEdit::new(Arc::clone(&state), "thread".into());
        tool.call(json!({"content": "# Plan\n- step"}), tool_context(&scope))
            .await
            .unwrap();
        let sources = state.sources("thread").await.unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].pinned);
        let file = root.join(sources::FOLDER).join(&sources[0].file);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# Plan\n- step");
        tool.call(json!({"content": "# Plan\n- more"}), tool_context(&scope))
            .await
            .unwrap();
        let after = state.sources("thread").await.unwrap();
        assert_eq!(after.len(), 1);
        assert!(after[0].pinned);
        assert_eq!(after[0].bytes, "# Plan\n- more".len() as u64);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# Plan\n- more");
    }

    #[tokio::test]
    async fn the_user_reads_rewrites_and_clears_the_same_plan_the_model_pins() {
        let state = Arc::new(temp_state("plan_user").await);
        state.create_thread("thread", "test").await.unwrap();
        let root = temp_dir("plan_user_scope");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(read(&state, &root, "thread").await.unwrap(), None);

        save(&state, &root, "thread", PLAN_NAME, "# Plan\n- a")
            .await
            .unwrap();
        assert_eq!(
            read(&state, &root, "thread").await.unwrap().as_deref(),
            Some("# Plan\n- a")
        );
        save(&state, &root, "thread", PLAN_NAME, "# Plan\n- b")
            .await
            .unwrap();
        let sources = state.sources("thread").await.unwrap();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].pinned);

        let too_big = "x".repeat(MAX_PLAN_BYTES + 1);
        assert!(
            save(&state, &root, "thread", PLAN_NAME, &too_big)
                .await
                .is_err()
        );

        clear(&state, &root, "thread").await.unwrap();
        assert_eq!(read(&state, &root, "thread").await.unwrap(), None);
        assert!(state.sources("thread").await.unwrap().is_empty());
    }

    #[test]
    fn a_plan_is_finished_when_every_box_is_ticked() {
        assert!(finished("# Plan\n- [x] a\n* [X] b\n1. [x] c\n"));
        assert!(!finished("# Plan\n- [x] a\n  - [ ] b\n"));
        assert!(!finished("2) [ ] only one, open"));
        // No boxes at all is a plan that says nothing of being done.
        assert!(!finished("# Plan\n- a\n- b\n[x] not an item"));
    }

    #[tokio::test]
    async fn old_plans_go_and_the_current_unfinished_one_stays() {
        let state = Arc::new(temp_state("plan_old").await);
        state.create_thread("thread", "test").await.unwrap();
        let root = temp_dir("plan_old_scope");
        std::fs::create_dir_all(&root).unwrap();
        save(&state, &root, "thread", "first.md", "- [ ] a")
            .await
            .unwrap();
        save(&state, &root, "thread", "done.md", "- [x] a")
            .await
            .unwrap();
        save(&state, &root, "thread", PLAN_NAME, "- [ ] b")
            .await
            .unwrap();
        // Written again, the first plan is the current one once more.
        save(&state, &root, "thread", "first.md", "- [ ] a\n- [ ] c")
            .await
            .unwrap();
        let plans = list(&state, &root, "thread").await.unwrap();
        let names: Vec<_> = plans.iter().map(|plan| plan.name.as_str()).collect();
        assert_eq!(names, ["first.md", PLAN_NAME, "done.md"]);
        assert!(plans[0].current && !plans[0].finished);
        assert!(plans[2].finished);
        assert_eq!(current_name(&state, "thread").await.unwrap(), "first.md");
        assert_eq!(
            read(&state, &root, "thread").await.unwrap().as_deref(),
            Some("- [ ] a\n- [ ] c")
        );

        assert_eq!(remove_old(&state, &root, "thread").await.unwrap(), 2);
        let left = state.sources("thread").await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "first.md");
        assert!(root.join(sources::FOLDER).join(&left[0].file).exists());

        // A finished current plan is old too.
        save(&state, &root, "thread", "first.md", "- [x] a")
            .await
            .unwrap();
        assert_eq!(remove_old(&state, &root, "thread").await.unwrap(), 1);
        assert!(state.sources("thread").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_tool_deletes_only_a_plan_of_its_own_chat() {
        let state = Arc::new(temp_state("plan_delete").await);
        state.create_thread("thread", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        let root = temp_dir("plan_delete_scope");
        std::fs::create_dir_all(&root).unwrap();
        let scope = ToolScope::new(&root, &root).unwrap();
        save(&state, &root, "other", PLAN_NAME, "- [ ] theirs")
            .await
            .unwrap();
        // An attached file is no plan, whatever its name.
        let attached = Source {
            source_id: "attached".into(),
            name: "notes.md".into(),
            file: "notes.md".into(),
            bytes: 8,
            pinned: false,
            delivered: false,
            plan: false,
        };
        state.add_source("thread", &attached).await.unwrap();
        let tool = PlanEdit::new(Arc::clone(&state), "thread".into());
        let delete = |title: Option<&str>| {
            let mut arguments = json!({"delete": true});
            if let Some(title) = title {
                arguments["title"] = json!(title);
            }
            tool.call(arguments, tool_context(&scope))
        };
        assert!(delete(None).await.is_err());
        assert!(delete(Some(PLAN_NAME)).await.is_err());
        assert!(delete(Some("notes.md")).await.is_err());

        tool.call(json!({"content": "- [ ] ours"}), tool_context(&scope))
            .await
            .unwrap();
        assert!(
            tool.call(
                json!({"delete": true, "content": "- [ ] x"}),
                tool_context(&scope)
            )
            .await
            .is_err()
        );
        // The current plan, unfinished, is still the work: it stays.
        let refused = delete(None).await.unwrap_err().to_string();
        assert!(refused.contains("not finished"), "{refused}");
        assert!(delete(Some(PLAN_NAME)).await.is_err());

        // Replaced by a newer plan, it goes; the newer one goes once finished.
        tool.call(
            json!({"content": "- [ ] next", "title": "next.md"}),
            tool_context(&scope),
        )
        .await
        .unwrap();
        delete(Some(PLAN_NAME)).await.unwrap();
        assert!(delete(None).await.is_err());
        tool.call(
            json!({"content": "- [x] next", "title": "next.md"}),
            tool_context(&scope),
        )
        .await
        .unwrap();
        delete(None).await.unwrap();
        let names = |thread: &'static str| {
            let state = Arc::clone(&state);
            async move {
                let sources = state.sources(thread).await.unwrap();
                sources.into_iter().map(|s| s.name).collect::<Vec<_>>()
            }
        };
        assert_eq!(names("thread").await, ["notes.md"]);
        assert_eq!(names("other").await, [PLAN_NAME]);
    }

    #[tokio::test]
    async fn a_plan_named_like_an_attached_file_leaves_the_file_be() {
        let state = Arc::new(temp_state("plan_attached").await);
        state.create_thread("thread", "test").await.unwrap();
        let root = temp_dir("plan_attached_scope");
        let folder = root.join(sources::FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("notes.md"), "mine").unwrap();
        let attached = Source {
            source_id: "attached".into(),
            name: "notes.md".into(),
            file: "notes.md".into(),
            bytes: 4,
            pinned: false,
            delivered: false,
            plan: false,
        };
        state.add_source("thread", &attached).await.unwrap();
        save(&state, &root, "thread", "notes.md", "- [x] plan")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("notes.md")).unwrap(),
            "mine"
        );
        assert_eq!(remove_old(&state, &root, "thread").await.unwrap(), 1);
        let left = state.sources("thread").await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].source_id, "attached");
    }
}
