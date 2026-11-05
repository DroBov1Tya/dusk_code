//! `plan_edit`: the pinned plan of the chat, one Markdown file among its sources. The
//! first call creates it; later calls rewrite it whole. Pinned sources come back with
//! every request, so the plan stays between the user and the model.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, ensure};
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

/// The name the plan goes by when the call gives none.
const DEFAULT_NAME: &str = "plan.md";

pub struct PlanEdit {
    state: Arc<State>,
    thread_id: String,
}

/// Flat, since providers want one object at the top of a schema.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// The whole plan as Markdown, not a diff.
    content: String,
    /// The name the plan goes by; defaults to plan.md.
    title: Option<String>,
}

impl PlanEdit {
    pub fn new(state: Arc<State>, thread_id: String) -> Self {
        Self { state, thread_id }
    }
}

impl Tool for PlanEdit {
    fn name(&self) -> &'static str {
        "plan_edit"
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Create or rewrite this chat's pinned plan: one Markdown file, pinned so the whole \
         of it comes back with every message. Give the whole plan as Markdown, not a patch; \
         the plan keeps one name, so call the tool again to update it."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            ensure!(!arguments.content.trim().is_empty(), "content is empty");
            let name = arguments
                .title
                .as_deref()
                .map(sources::clean_name)
                .unwrap_or_else(|| DEFAULT_NAME.to_owned());
            let bytes = arguments.content.as_bytes();
            let folder = context.scope.workspace().join(sources::FOLDER);
            let existing = self
                .state
                .sources(&self.thread_id)
                .await?
                .into_iter()
                .find(|source| source.name == name);
            match existing {
                Some(source) => {
                    let path = folder.join(&source.file);
                    write(&path, bytes).await?;
                    self.state
                        .set_source_bytes(&self.thread_id, &source.source_id, bytes.len() as u64)
                        .await?;
                    Ok(format!(
                        "the pinned plan {name} is updated ({} bytes)",
                        bytes.len()
                    ))
                }
                None => {
                    crate::init::create_private_dir(&folder)?;
                    let source_id = Uuid::now_v7().to_string();
                    // The end of the ID differs between sources added together.
                    let tail = source_id[source_id.len() - 6..].to_owned();
                    let source = Source {
                        source_id,
                        name: name.clone(),
                        file: format!("{tail}-{name}"),
                        bytes: bytes.len() as u64,
                        pinned: false,
                        delivered: false,
                    };
                    write(&folder.join(&source.file), bytes).await?;
                    self.state.add_source(&self.thread_id, &source).await?;
                    // `add_source` records new sources unpinned; the plan starts pinned.
                    self.state
                        .pin_source(&self.thread_id, &source.source_id, true)
                        .await?;
                    Ok(format!(
                        "the pinned plan {name} is created ({} bytes, source #{})",
                        bytes.len(),
                        &source.source_id[source.source_id.len() - 8..]
                    ))
                }
            }
        })
    }
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
}
