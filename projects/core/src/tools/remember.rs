//! `remember`: what the agent knows about its owner across conversations, kept in
//! MEMORY.md. The memory is shown in the tool's description, so every request carries
//! it while the tool is on, and switching the tool off hides it as well. Each call
//! writes the whole memory again, which keeps it a short summary rather than a log.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    builtin::parse_arguments,
    tool::{Tool, ToolContext},
};
use crate::BoxFuture;

/// Every request carries the memory, so it stays small.
const MAX_MEMORY_CHARS: usize = 2000;

pub struct Remember {
    file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberArguments {
    memory: String,
}

impl Remember {
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    /// The memory as it is on disk; a missing or unreadable file is an empty memory.
    fn read(&self) -> String {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(_) => return "(MEMORY.md cannot be read)".into(),
        };
        text.trim().chars().take(MAX_MEMORY_CHARS).collect()
    }

    /// Written beside the file and moved over it, so a failed write keeps the old one.
    fn write(&self, memory: &str) -> Result<()> {
        let temporary = self.file.with_extension("md.saving");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .context("cannot write the memory")?;
        file.write_all(memory.as_bytes())
            .and_then(|()| file.sync_all())
            .context("cannot write the memory")?;
        fs::rename(&temporary, &self.file).context("cannot replace the memory")
    }
}

impl Tool for Remember {
    fn name(&self) -> &'static str {
        "remember"
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        let memory = self.read();
        let memory = if memory.is_empty() {
            "(empty)".to_owned()
        } else {
            memory
        };
        format!(
            "Your memory of the user, kept across conversations. When the user states a \
             lasting preference or a fact about how they work, or asks you to remember \
             something, write the whole memory again with it: the shortest summary, one \
             fact per line, merged with what is there, outdated lines dropped. Never store \
             secrets, credentials, or anything about other people. An empty text forgets \
             everything. At most {MAX_MEMORY_CHARS} characters.\n\
             Current memory:\n{memory}"
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "memory": {
                    "type": "string",
                    "description": "The whole new memory, replacing the current one."
                }
            },
            "required": ["memory"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: RememberArguments = parse_arguments(arguments)?;
            let memory = arguments.memory.trim();
            let chars = memory.chars().count();
            ensure!(
                chars <= MAX_MEMORY_CHARS,
                "the memory is {chars} characters, over {MAX_MEMORY_CHARS}; make it shorter"
            );
            self.write(memory)?;
            Ok(if memory.is_empty() {
                "memory cleared".to_owned()
            } else {
                format!("memory saved, {chars} characters")
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, testing::temp_dir, tools::ToolScope};

    #[tokio::test]
    async fn the_memory_is_replaced_whole_and_shown_in_the_description() {
        let directory = temp_dir("remember");
        fs::create_dir_all(&directory).unwrap();
        let tool = Remember::new(directory.join("MEMORY.md"));
        let scope = ToolScope::new(&directory, &directory).unwrap();
        let config = Config::parse(b"{}").unwrap();
        let context = ToolContext {
            scope: &scope,
            config: &config,
            progress: None,
        };
        assert!(tool.description(&context).ends_with("(empty)"));
        tool.call(json!({"memory": "- prefers Russian"}), context)
            .await
            .unwrap();
        tool.call(json!({"memory": "- prefers Russian\n- uses tmux"}), context)
            .await
            .unwrap();
        assert!(
            tool.description(&context)
                .ends_with("- prefers Russian\n- uses tmux")
        );
        let long = "x".repeat(MAX_MEMORY_CHARS + 1);
        assert!(tool.call(json!({"memory": long}), context).await.is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(directory.join("MEMORY.md"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
