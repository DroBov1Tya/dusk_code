//! First-start initialization. Creates the missing parts of the layout in `paths`
//! with private permissions and never modifies existing files.

pub mod logs;
pub mod paths;

use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail, ensure};

use tracing::{info, warn};

use crate::{
    catalog::modes,
    init::paths::Paths,
    release::builtin::{self, LOCK_FILE, Synced},
};

const CONFIG_TEMPLATE: &str = "\
# dusk_code provider settings. Keep this file private.
# Add a provider when you are ready to connect, for example:
#
# providers:
#   main:
#     kind: openai_compatible
#     base_url: 'https://api.example.com/v1'
#     api_key: 'sk-...' # optional for local providers
providers: {}

thread:
  # full:   store user messages and the whole model output (reasoning, tool calls, answer)
  # answer: store user messages and model answers only
  record: full

limits:
  # A request larger than this is not sent; the context is compacted at 80% of it,
  # or of the model window when that is smaller.
  max_input_tokens: 100000
  # The model is stopped after writing this many tokens in one call.
  max_output_tokens: 100000
  # Maximum number of subagents working at once across all threads.
  max_agents_at_work: 8

# Tools and skills are on unless switched off here or with :off in a client, for example:
# tools:
#   run_command: false
# skills:
#   git/release-notes: false
#
# The core rewrites this file when a setting changes at runtime; only its own header
# comment is kept then.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialization {
    /// True on first start: the config is a template without providers.
    pub config_created: bool,
    /// What this start did to the prompts and skills dusk ships.
    pub builtin: Synced,
}

pub fn initialize(paths: &Paths) -> Result<Initialization> {
    create_private_dir(paths.config_root())?;
    create_private_dir(&paths.records_dir())?;
    create_private_dir(paths.workspace_root())?;
    move_into_rules(paths)?;
    create_private_dir(&paths.skills_dir())?;
    create_private_dir(&paths.agents_dir())?;
    let config_created = ensure_config(&paths.config_file())?;
    create_private_dir(&paths.modes_dir())?;
    move_legacy_main_prompt(paths)?;
    // Shipped files out of date only make the agent know less; they never stop the start.
    let builtin = match builtin::sync(paths) {
        Ok(synced) => {
            for (what, files) in [("added", &synced.written), ("updated", &synced.updated)] {
                if !files.is_empty() {
                    info!("shipped files {what}: {}", files.join(", "));
                }
            }
            synced
        }
        Err(error) => {
            warn!("cannot bring the shipped files up to date: {error:#}");
            Synced::default()
        }
    };
    // SQLite would create the file with umask permissions; an empty file is a valid new database.
    create_private_file(&paths.state_db(), b"")?;
    Ok(Initialization {
        config_created,
        builtin,
    })
}

/// What sat in the config root before `rules/`, and where each goes in it, folders first.
const BEFORE_RULES: [(&str, &str); 7] = [
    ("agents", "agents"),
    ("skills", "skills"),
    ("modes", "modes"),
    ("internal", "prompts"),
    ("COMPACT.md", "prompts/COMPACT.md"),
    ("FORK.md", "prompts/FORK.md"),
    ("MEMORY.md", "MEMORY.md"),
];

/// Moves an older config folder's files into `rules/`. Each move is whole, so a stop half
/// way goes on at the next start; something already in place is never replaced. The record
/// of shipped files goes last with its paths rewritten: without it they would all read as
/// the user's own, and updates would stop.
fn move_into_rules(paths: &Paths) -> Result<()> {
    let (root, rules) = (paths.config_root(), paths.rules_root());
    create_private_dir(&rules)?;
    for (old, new) in BEFORE_RULES {
        let (from, to) = (root.join(old), rules.join(new));
        if fs::symlink_metadata(&from).is_err() {
            continue;
        }
        if fs::symlink_metadata(&to).is_ok() {
            warn!("{old} stays in the config folder: {new} is already in rules");
            continue;
        }
        if let Some(parent) = to.parent() {
            create_private_dir(parent)?;
        }
        fs::rename(&from, &to).with_context(|| format!("cannot move {old} into rules"))?;
        info!("{old} moved to rules/{new}");
    }
    let (old, new) = (root.join(LOCK_FILE), rules.join(LOCK_FILE));
    if let (Ok(text), false) = (fs::read_to_string(&old), new.exists()) {
        let lock: BTreeMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
        let moved: BTreeMap<String, String> = lock
            .into_iter()
            .map(|(path, hash)| (moved_path(&path), hash))
            .collect();
        fs::write(&new, serde_json::to_string_pretty(&moved)?)
            .context("cannot write the shipped file record")?;
        fs::remove_file(&old).context("cannot remove the old shipped file record")?;
    }
    Ok(())
}

/// A shipped file's path from the config root as it is from the rules folder now.
fn moved_path(path: &str) -> String {
    match path.strip_prefix("internal/") {
        Some(rest) => format!("prompts/{rest}"),
        None if matches!(path, "COMPACT.md" | "FORK.md") => format!("prompts/{path}"),
        None => path.to_owned(),
    }
}

/// Before modes, MAIN.md sat in the config root. A MAIN.md the user wrote becomes the
/// orchestrator's prompt, as it was written to coordinate; the untouched template is
/// MAIN mode's. The file is moved, never rewritten, and only into a free place.
fn move_legacy_main_prompt(paths: &Paths) -> Result<()> {
    let legacy = paths.legacy_main_prompt();
    let Ok(text) = fs::read_to_string(&legacy) else {
        return Ok(());
    };
    let template = match modes::template(modes::MAIN) {
        Ok(Some(template)) => template,
        _ => {
            warn!(
                "cannot classify legacy MAIN.md without its resource template; keeping it in place"
            );
            return Ok(());
        }
    };
    let mode = if text.trim() == template.trim() {
        modes::MAIN
    } else {
        modes::ORCHESTRATOR
    };
    let target = modes::file(&paths.modes_dir(), mode);
    // The mode has its prompt: the old file is kept beside it, where it is no mode.
    let target = match fs::symlink_metadata(&target) {
        Ok(_) => target.with_extension("md.old"),
        Err(_) => target,
    };
    if fs::symlink_metadata(&target).is_ok() {
        return Ok(());
    }
    fs::rename(&legacy, &target).context("cannot move MAIN.md into modes")?;
    info!("MAIN.md moved to modes/{mode}.md");
    Ok(())
}

pub fn create_private_dir(directory: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|_| anyhow!("cannot create application directory"))?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| anyhow!("cannot inspect application directory"))?;
    ensure!(
        metadata.is_dir(),
        "application directory must not be a symlink"
    );
    Ok(())
}

/// Creates the config template if the file is missing. Returns whether it was created.
fn ensure_config(path: &Path) -> Result<bool> {
    let directory = path.parent().context("config directory is missing")?;
    create_private_dir(directory)?;
    create_private_file(path, CONFIG_TEMPLATE.as_bytes())
}

/// Creates a mode 0600 file with `contents` if the path is free. Returns whether it was created.
fn create_private_file(path: &Path, contents: &[u8]) -> Result<bool> {
    let mut options = OpenOptions::new();
    // create_new never follows or replaces an existing path, including symlinks.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(_) => bail!("cannot create application file"),
    };
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        bail!("cannot write application file");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_paths;

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        temp_paths(name).config_file()
    }

    #[cfg(unix)]
    #[test]
    fn initialize_creates_private_layout() {
        use std::os::unix::fs::PermissionsExt;

        let paths = temp_paths("layout");
        assert!(initialize(&paths).unwrap().config_created);
        assert!(!initialize(&paths).unwrap().config_created);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(paths.config_root()), 0o700);
        assert_eq!(mode(paths.workspace_root()), 0o700);
        assert_eq!(mode(&paths.records_dir()), 0o700);
        assert_eq!(mode(&paths.config_file()), 0o600);
        assert_eq!(mode(&paths.main_prompt()), 0o600);
        assert_eq!(mode(&paths.compact_prompt()), 0o600);
        assert_eq!(mode(&paths.skills_dir()), 0o700);
        assert_eq!(mode(&paths.agents_dir()), 0o700);
        assert_eq!(mode(&paths.state_db()), 0o600);
    }

    #[test]
    fn an_old_config_folder_moves_into_rules_and_stays_up_to_date() {
        let paths = temp_paths("rules_move");
        let root = paths.config_root();
        fs::create_dir_all(root.join("agents/mine")).unwrap();
        fs::create_dir_all(root.join("internal")).unwrap();
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::write(root.join("agents/mine/AGENT.md"), "mine").unwrap();
        fs::write(root.join("MEMORY.md"), "remembered").unwrap();
        let old_compact = "older compaction text";
        fs::write(root.join("COMPACT.md"), old_compact).unwrap();
        // COMPACT.md is an old shipped file the user never changed.
        let hash: String = <sha2::Sha256 as sha2::Digest>::digest(old_compact.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let lock = format!("{{\"COMPACT.md\": \"{hash}\"}}");
        fs::write(root.join(LOCK_FILE), lock).unwrap();
        initialize(&paths).unwrap();
        let rules = paths.rules_root();
        assert_eq!(
            fs::read_to_string(rules.join("agents/mine/AGENT.md")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(paths.memory_file()).unwrap(),
            "remembered"
        );
        for gone in [
            "agents",
            "skills",
            "internal",
            "COMPACT.md",
            "MEMORY.md",
            LOCK_FILE,
        ] {
            assert!(!root.join(gone).exists(), "{gone}");
        }
        assert_ne!(
            fs::read_to_string(paths.compact_prompt()).unwrap(),
            old_compact
        );
        assert!(!rules.join("prompts/COMPACT.md.new").exists());
        initialize(&paths).unwrap();
    }

    #[test]
    fn a_file_already_in_rules_is_not_replaced() {
        let paths = temp_paths("rules_keep");
        fs::create_dir_all(paths.rules_root()).unwrap();
        fs::write(paths.memory_file(), "newer").unwrap();
        fs::write(paths.config_root().join("MEMORY.md"), "older").unwrap();
        initialize(&paths).unwrap();
        assert_eq!(fs::read_to_string(paths.memory_file()).unwrap(), "newer");
        assert!(paths.config_root().join("MEMORY.md").exists());
    }

    #[test]
    fn a_written_main_prompt_becomes_the_orchestrator() {
        let paths = temp_paths("legacy_main");
        fs::create_dir_all(paths.config_root()).unwrap();
        fs::write(paths.legacy_main_prompt(), "Coordinate the agents.").unwrap();
        initialize(&paths).unwrap();
        assert!(!paths.legacy_main_prompt().exists());
        let orchestrator = modes::file(&paths.modes_dir(), modes::ORCHESTRATOR);
        assert_eq!(
            fs::read_to_string(orchestrator).unwrap(),
            "Coordinate the agents."
        );
        let main = fs::read_to_string(paths.main_prompt()).unwrap();
        assert_eq!(main, modes::template(modes::MAIN).unwrap().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink() {
        let path = temp_config_path("symlink");
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory).unwrap();
        let target = directory.join("target.yml");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(!ensure_config(&path).unwrap());
        assert!(!target.exists());
    }
}
