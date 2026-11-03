//! `remember` and `remember_project`: what the agent keeps across conversations. The
//! first holds the owner's preferences in MEMORY.md, the second the facts of the project
//! in the working directory, in `projects/<folder>/MEMORY.md` of the rules folder, out of
//! the repository and out of the tools' reach. The memory is shown in the tool's
//! description, so every request carries it while it is on in the settings; switched
//! off, the tool is gone and the memory with it. Each call writes the whole memory
//! again, which keeps it a short summary rather than a log.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::BoxFuture;
use crate::init::create_private_dir;
use crate::tools::{
    builtin::parse_arguments,
    schema,
    tool::{Tool, ToolContext},
};

/// Of a project's folder name, what names its memory folder before the hash.
const NAME_CHARS: usize = 40;

#[derive(Clone, Copy)]
enum Kind {
    User,
    Project,
}

pub struct Remember {
    rules: PathBuf,
    kind: Kind,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RememberArguments {
    /// The whole new memory, replacing the current one.
    memory: String,
}

impl Remember {
    /// The owner's memory, kept in the rules folder.
    pub fn user(rules: PathBuf) -> Self {
        Self {
            rules,
            kind: Kind::User,
        }
    }

    /// The working directory's memory, kept in the rules folder too.
    pub fn project(rules: PathBuf) -> Self {
        Self {
            rules,
            kind: Kind::Project,
        }
    }

    fn file(&self, cwd: &Path) -> PathBuf {
        match self.kind {
            Kind::User => self.rules.join("MEMORY.md"),
            Kind::Project => self
                .rules
                .join("projects")
                .join(project_folder(cwd))
                .join("MEMORY.md"),
        }
    }

    fn prompt(&self) -> &'static str {
        match self.kind {
            Kind::User => "MEMORY.md",
            Kind::Project => "WORK_MEMORY.md",
        }
    }

    fn settings(&self, context: &ToolContext<'_>) -> (bool, usize) {
        match self.kind {
            Kind::User => (
                context.config.memory().enabled,
                context.config.memory().max_chars,
            ),
            Kind::Project => (
                context.config.work_history().enabled,
                context.config.work_history().max_chars,
            ),
        }
    }
}

/// A readable name with a hash of the whole path, so two projects of one name never
/// share a memory.
fn project_folder(cwd: &Path) -> String {
    let name: String = cwd
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .map(|char| if char.is_alphanumeric() { char } else { '-' })
        .take(NAME_CHARS)
        .collect();
    let hash = Sha256::digest(cwd.as_os_str().as_encoded_bytes());
    let hash: String = hash[..6].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{name}-{hash}")
}

/// The memory as it is on disk, cut to `max_chars`; a missing file is an empty memory.
fn read(file: &Path, max_chars: usize) -> String {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(_) => return "(the memory cannot be read)".into(),
    };
    text.trim().chars().take(max_chars).collect()
}

/// Written beside the file and moved over it, so a failed write keeps the old one.
fn write(file: &Path, memory: &str) -> Result<()> {
    if let Some(folder) = file.parent() {
        create_private_dir(folder)?;
    }
    let temporary = file.with_extension("md.saving");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut saving = options
        .open(&temporary)
        .context("cannot write the memory")?;
    saving
        .write_all(memory.as_bytes())
        .and_then(|()| saving.sync_all())
        .context("cannot write the memory")?;
    fs::rename(&temporary, file).context("cannot replace the memory")
}

impl Tool for Remember {
    fn name(&self) -> &'static str {
        match self.kind {
            Kind::User => "remember",
            Kind::Project => "remember_project",
        }
    }

    fn switchable(&self) -> bool {
        false
    }

    fn available(&self, context: &ToolContext<'_>) -> bool {
        self.settings(context).0
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let (_, max_chars) = self.settings(context);
        let memory = read(&self.file(context.scope.cwd()), max_chars);
        let memory = if memory.is_empty() {
            "(empty)".to_owned()
        } else {
            memory
        };
        let prompt = self.rules.join("prompts").join(self.prompt());
        let template = crate::runtime::chat::thread_start::read_prompt(&prompt)
            .ok()
            .flatten()
            .or_else(|| {
                crate::release::resources::text(&format!("internal/{}", self.prompt())).ok()
            });
        let Some(template) = template else {
            return "Memory instructions are unavailable; restore the versioned resources.".into();
        };
        // Substitute once so a memory containing placeholders stays literal.
        template
            .replace("{max_chars}", &max_chars.to_string())
            .replace("{memory}", &memory)
    }

    fn parameters(&self) -> Value {
        schema::of::<RememberArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let (enabled, max_chars) = self.settings(&context);
            ensure!(enabled, "this memory is switched off in the settings");
            let arguments: RememberArguments = parse_arguments(arguments)?;
            let memory = arguments.memory.trim();
            let chars = memory.chars().count();
            ensure!(
                chars <= max_chars,
                "the memory is {chars} characters, over {max_chars}; make it shorter"
            );
            write(&self.file(context.scope.cwd()), memory)?;
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
    use serde_json::json;

    #[tokio::test]
    async fn the_memory_is_replaced_whole_and_shown_in_the_description() {
        let directory = temp_dir("remember");
        fs::create_dir_all(&directory).unwrap();
        let tool = Remember::user(directory.clone());
        let scope = ToolScope::new(&directory, &directory).unwrap();
        let config = Config::parse(b"{}").unwrap();
        let context = ToolContext {
            scope: &scope,
            config: &config,
            progress: None,
            approvals: None,
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

    #[tokio::test]
    async fn each_project_keeps_its_own_memory_and_it_starts_switched_off() {
        let root = temp_dir("remember-project");
        let (rules, one, two) = (root.join("rules"), root.join("a/app"), root.join("b/app"));
        for folder in [&rules, &one, &two] {
            fs::create_dir_all(folder).unwrap();
        }
        let tool = Remember::project(rules.clone());
        let off = Config::parse(b"{}").unwrap();
        let on = Config::parse(b"work_history:\n  enabled: true\n").unwrap();
        let (one, two) = (
            ToolScope::new(&one, &one).unwrap(),
            ToolScope::new(&two, &two).unwrap(),
        );
        let context = |scope, config| ToolContext {
            scope,
            config,
            progress: None,
            approvals: None,
        };
        assert!(!tool.available(&context(&one, &off)));
        assert!(
            tool.call(json!({"memory": "x"}), context(&one, &off))
                .await
                .is_err()
        );
        tool.call(json!({"memory": "- cargo test"}), context(&one, &on))
            .await
            .unwrap();
        assert!(
            tool.description(&context(&one, &on))
                .ends_with("- cargo test")
        );
        assert!(tool.description(&context(&two, &on)).ends_with("(empty)"));
        assert!(!fs::exists(root.join("a/app/MEMORY.md")).unwrap());
    }
}
