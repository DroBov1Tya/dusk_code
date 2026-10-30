//! `remember`: what the agent knows about its owner across conversations, kept in
//! MEMORY.md. The memory is shown in the tool's description, so every request carries
//! it while memory is on in the settings; switched off, the tool is gone and the memory
//! with it. Each call writes the whole memory again, which keeps it a short summary
//! rather than a log. Its size is `memory.max_chars` in config.yml.

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

    /// The memory as it is on disk, cut to `max_chars`; a missing or unreadable file is
    /// an empty memory.
    fn read(&self, max_chars: usize) -> String {
        let text = match fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(_) => return "(MEMORY.md cannot be read)".into(),
        };
        text.trim().chars().take(max_chars).collect()
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

    fn switchable(&self) -> bool {
        false
    }

    fn available(&self, context: &ToolContext<'_>) -> bool {
        context.config.memory().enabled
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let max_chars = context.config.memory().max_chars;
        let memory = self.read(max_chars);
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
             everything. At most {max_chars} characters.\n\
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

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let memory = context.config.memory();
            ensure!(memory.enabled, "memory is switched off in the settings");
            let max_chars = memory.max_chars;
            let arguments: RememberArguments = parse_arguments(arguments)?;
            let memory = arguments.memory.trim();
            let chars = memory.chars().count();
            ensure!(
                chars <= max_chars,
                "the memory is {chars} characters, over {max_chars}; make it shorter"
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
        let long = "x".repeat(config.memory().max_chars + 1);
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
