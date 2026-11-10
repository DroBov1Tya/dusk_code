//! A project's environment variables: set by the user, given to the commands that run in
//! its folder, and never shown again; clients get the names only.

use anyhow::{Result, ensure};
use dusk_protocol::project::{MAX_ENV_NAME_CHARS, MAX_ENV_VALUE_BYTES, MAX_ENV_VARS};

use crate::state::State;

/// Names the program keeps for itself.
const RESERVED_PREFIX: &str = "DUSK_";
/// Names that change how a program is found or loaded, or how the shell reads its
/// input: a project holds secrets for commands, not a way to swap their tools.
const LOADER_PREFIXES: [&str; 2] = ["LD_", "DYLD_"];
const SHELL_NAMES: [&str; 6] = ["PATH", "IFS", "SHELL", "ENV", "BASH_ENV", "NODE_OPTIONS"];

pub async fn names(state: &State, id: &str) -> Result<Vec<String>> {
    super::find(state, id).await?;
    state.project_env_names(id).await
}

pub async fn set(state: &State, id: &str, name: &str, value: &str) -> Result<Vec<String>> {
    super::find(state, id).await?;
    ensure!(
        is_name(name),
        "a name is letters, digits and _, not starting with a digit"
    );
    ensure!(
        !name.to_ascii_uppercase().starts_with(RESERVED_PREFIX),
        "names starting with {RESERVED_PREFIX} are kept for the program"
    );
    let upper = name.to_ascii_uppercase();
    ensure!(
        !SHELL_NAMES.contains(&upper.as_str())
            && !LOADER_PREFIXES
                .iter()
                .any(|prefix| upper.starts_with(prefix)),
        "{name} changes how commands load programs and cannot be set here"
    );
    ensure!(
        value.len() <= MAX_ENV_VALUE_BYTES && !value.contains('\0'),
        "a value is text of at most {MAX_ENV_VALUE_BYTES} bytes"
    );
    let known = state.project_env_names(id).await?;
    ensure!(
        known.len() < MAX_ENV_VARS || known.iter().any(|known| known == name),
        "a project holds at most {MAX_ENV_VARS} variables"
    );
    state.set_project_env(id, name, value).await?;
    state.project_env_names(id).await
}

pub async fn remove(state: &State, id: &str, name: &str) -> Result<Vec<String>> {
    super::find(state, id).await?;
    state.remove_project_env(id, name).await?;
    state.project_env_names(id).await
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_ENV_NAME_CHARS
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_state;

    #[tokio::test]
    async fn only_plain_names_and_sane_values_are_kept() {
        let state = temp_state("project_env_rules").await;
        state.create_project("p", "P", "", "/work/p").await.unwrap();
        for name in [
            "",
            "1A",
            "A-B",
            "A B",
            "A=B",
            "dusk_cwd_file",
            "DUSK_X",
            "é",
        ] {
            assert!(set(&state, "p", name, "v").await.is_err(), "{name:?}");
        }
        assert!(set(&state, "p", "A", "a\0b").await.is_err());
        let long = "x".repeat(MAX_ENV_VALUE_BYTES + 1);
        assert!(set(&state, "p", "A", &long).await.is_err());
        assert!(set(&state, "missing", "A", "v").await.is_err());

        assert_eq!(set(&state, "p", "_ok1", "v").await.unwrap(), ["_ok1"]);
        for index in 1..MAX_ENV_VARS {
            set(&state, "p", &format!("V{index}"), "v").await.unwrap();
        }
        assert!(set(&state, "p", "ONE_TOO_MANY", "v").await.is_err());
        // An existing name may still change at the limit.
        set(&state, "p", "_ok1", "new").await.unwrap();
        assert_eq!(
            remove(&state, "p", "_ok1").await.unwrap().len(),
            MAX_ENV_VARS - 1
        );
    }
}
