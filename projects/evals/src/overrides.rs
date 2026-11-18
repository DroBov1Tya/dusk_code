//! `--set NAME=VALUE`: a key of the run's config.yml changed for that run only, so a
//! harness mechanism can be measured on and off without editing the user's file.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Map, Value};

/// Sections a run may change. Deny by default: providers, MCP servers, plugins and voice
/// hold keys and addresses that must not end up in a report.
const SETTABLE: &[&str] = &[
    "agents",
    "approvals",
    "effort",
    "limits",
    "memory",
    "mode",
    "model",
    "skill_packs",
    "skills",
    "thread",
    "tools",
    "version_control",
    "work_history",
];

/// Switches by name, where the core takes any name: only one already in the config can
/// be set, so a misspelt name is an error and not a switch that does nothing.
const NAMED: &[&str] = &["agents", "skill_packs", "skills", "tools"];

#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    /// Dotted, such as `limits.max_attempts`.
    pub name: String,
    /// As given, for the report.
    pub value: String,
    scalar: Value,
}

impl Setting {
    /// Reads `NAME=VALUE`, where VALUE is one YAML value such as `false`, `3` or `low`.
    /// Whether a section's key exists is the core's to say: it refuses unknown keys.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (name, value) = text.split_once('=').ok_or("expected NAME=VALUE")?;
        let word = |key: &str| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        };
        if !name.split('.').all(word) {
            return Err(format!("`{name}` is not a dotted config key"));
        }
        let section = name.split('.').next().unwrap_or_default();
        if !SETTABLE.contains(&section) {
            return Err(format!(
                "`{section}` cannot be set; one of: {}",
                SETTABLE.join(", ")
            ));
        }
        let scalar = match serde_saphyr::from_str::<Value>(value) {
            Ok(Value::Object(_) | Value::Array(_)) => {
                return Err("VALUE must be one value; set nested keys one by one".into());
            }
            Ok(scalar) if !value.trim().is_empty() => scalar,
            _ => return Err(format!("`{value}` is not a YAML value")),
        };
        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
            scalar,
        })
    }
}

/// The settings as a report keeps them.
pub fn recorded(settings: &[Setting]) -> BTreeMap<String, String> {
    settings
        .iter()
        .map(|setting| (setting.name.clone(), setting.value.clone()))
        .collect()
}

/// `config` with every setting applied. The parser's own error is left out: it may
/// quote the file, and the file holds API keys.
pub fn apply(config: &[u8], settings: &[Setting]) -> Result<String> {
    let mut tree: Value = serde_saphyr::from_slice(config).map_err(|error| {
        let at = error
            .location()
            .map(|at| format!(" at line {}", at.line()))
            .unwrap_or_default();
        anyhow!("the config is not valid YAML{at}")
    })?;
    if tree.is_null() {
        tree = Value::Object(Map::new());
    }
    let mut seen = BTreeSet::new();
    for setting in settings {
        ensure!(
            seen.insert(&setting.name),
            "--set {} is given twice",
            setting.name
        );
        set(&mut tree, setting)?;
    }
    serde_saphyr::to_string(&tree).context("cannot write the config")
}

fn set(tree: &mut Value, setting: &Setting) -> Result<()> {
    let keys: Vec<&str> = setting.name.split('.').collect();
    if let [section, entry, ..] = keys[..]
        && NAMED.contains(&section)
    {
        ensure!(
            tree.get(section)
                .and_then(|names| names.get(entry))
                .is_some(),
            "{section}.{entry} is not in the config; the core takes any name there, so add it \
             to the --config file first"
        );
    }
    let (last, path) = keys.split_last().context("an empty name")?;
    let mut node = tree;
    for key in path {
        if node.is_null() {
            *node = Value::Object(Map::new());
        }
        node = match node {
            Value::Object(map) => map.entry(*key).or_insert_with(|| Value::Object(Map::new())),
            _ => bail!("{} goes inside a value that is not a section", setting.name),
        };
    }
    if node.is_null() {
        *node = Value::Object(Map::new());
    }
    match node {
        Value::Object(map) => {
            map.insert((*last).to_owned(), setting.scalar.clone());
            Ok(())
        }
        _ => bail!("{} goes inside a value that is not a section", setting.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Setting {
        Setting::parse(text).expect("a valid setting")
    }

    fn applied(config: &str, settings: &[&str]) -> Result<Value> {
        let settings: Vec<Setting> = settings.iter().map(|text| parsed(text)).collect();
        let text = apply(config.as_bytes(), &settings)?;
        Ok(serde_saphyr::from_str(&text)?)
    }

    #[test]
    fn a_setting_is_a_dotted_key_and_one_value() {
        let setting = parsed("limits.max_attempts=1");
        assert_eq!(setting.name, "limits.max_attempts");
        assert_eq!(setting.scalar, Value::from(1));
        assert_eq!(parsed("memory.enabled=false").scalar, Value::from(false));
        assert_eq!(parsed("effort=low").scalar, Value::from("low"));
        for wrong in [
            "limits.max_attempts",
            "=1",
            "limits..max_attempts=1",
            "limits.max attempts=1",
            "limits.max_attempts=",
            "limits.max_attempts={a: 1}",
            "limits.max_attempts=[1, 2]",
            "providers.openai.api_key=sk-1",
            "plugins.telegram.token=x",
            "unknown=1",
        ] {
            assert!(Setting::parse(wrong).is_err(), "{wrong} was taken");
        }
    }

    #[test]
    fn settings_change_only_their_keys() {
        let config = "model: a/b\nlimits:\n  max_attempts: 6\ntools:\n  run_command: true\n";
        let tree = applied(
            config,
            &[
                "limits.max_attempts=1",
                "memory.enabled=false",
                "tools.run_command=false",
            ],
        )
        .expect("applied");
        assert_eq!(tree["model"], "a/b");
        assert_eq!(tree["limits"]["max_attempts"], 1);
        assert_eq!(tree["memory"]["enabled"], false);
        assert_eq!(tree["tools"]["run_command"], false);
        assert!(applied("", &["effort=low"]).is_ok());
    }

    #[test]
    fn unknown_names_and_twice_set_keys_are_errors() {
        let config = "tools:\n  run_command: true\nmodel: a/b\n";
        assert!(applied(config, &["tools.run_comand=false"]).is_err());
        assert!(applied(config, &["skills.nothing=false"]).is_err());
        assert!(applied(config, &["model.inner=1"]).is_err());
        assert!(applied(config, &["effort=low", "effort=high"]).is_err());
        assert!(applied("model: [", &["effort=low"]).is_err());
    }
}
