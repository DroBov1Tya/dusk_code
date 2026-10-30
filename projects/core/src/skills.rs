//! Skills as a client manages them: every folder under `skills/` with a `SKILL.md`, its
//! switch, and its text to read and edit. Skills are read from disk each time, so a
//! skill added or edited by hand applies at once and nothing is kept elsewhere.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde_yaml::Value;

use crate::{
    config::Config,
    graph::{Graph, SKILL_FILE, check_node, read_text, split_front_matter},
    paths::is_plain_folder_name,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Why the skill cannot be used as it is; it can still be opened and fixed.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillText {
    pub name: String,
    /// The front matter fields with plain values, in file order, such as `description`
    /// and `license`.
    pub metadata: Vec<(String, String)>,
    /// The whole file, front matter included.
    pub text: String,
}

/// Every skill, the ones that cannot be read included, by name.
pub fn list(directory: &Path, config: &Config) -> Vec<SkillInfo> {
    let graph = Graph::load(directory, SKILL_FILE);
    let mut skills: Vec<SkillInfo> = graph
        .nodes()
        .map(|node| SkillInfo {
            name: node.name.clone(),
            description: node.description.clone(),
            enabled: config.skill_enabled(&node.name),
            problem: None,
        })
        .collect();
    for problem in graph.problems() {
        if graph.get(&problem.node).is_none() {
            skills.push(SkillInfo {
                name: problem.node.clone(),
                description: String::new(),
                enabled: config.skill_enabled(&problem.node),
                problem: Some(problem.reason.clone()),
            });
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The skill's file as it is on disk now.
pub fn read(directory: &Path, name: &str) -> Result<SkillText> {
    let text = read_text(&file(directory, name)?)?;
    Ok(SkillText {
        name: name.to_owned(),
        metadata: metadata(&text),
        text,
    })
}

/// Replaces the skill's file with `text` once it reads as a skill. The file is written
/// beside the old one and moved over it, so a failed write leaves the old one whole.
pub fn save(directory: &Path, name: &str, text: &str) -> Result<SkillText> {
    let path = file(directory, name)?;
    ensure!(path.is_file(), "skill {name} does not exist");
    check_node(text, name)?;
    let temporary = path.with_extension("md.saving");
    let mut out = fs::File::create(&temporary).context("cannot write the skill")?;
    out.write_all(text.as_bytes())
        .and_then(|()| out.sync_all())
        .context("cannot write the skill")?;
    fs::rename(&temporary, &path).context("cannot replace the skill")?;
    read(directory, name)
}

/// The skill's file by name; the name is folder names joined by `/`, never a way out of
/// the directory.
fn file(directory: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && name.split('/').all(is_plain_folder_name),
        "invalid skill name {name:?}"
    );
    Ok(directory.join(name).join(SKILL_FILE))
}

fn metadata(text: &str) -> Vec<(String, String)> {
    let Ok((front, _)) = split_front_matter(text) else {
        return Vec::new();
    };
    let Ok(Value::Mapping(fields)) = serde_yaml::from_str::<Value>(front) else {
        return Vec::new();
    };
    fields
        .into_iter()
        .filter_map(|(key, value)| Some((plain(&key)?, plain(&value)?)))
        .collect()
}

/// A scalar as text, a list of scalars joined; anything deeper is left out.
fn plain(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.split_whitespace().collect::<Vec<_>>().join(" ")),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Sequence(items) => items
            .iter()
            .map(plain)
            .collect::<Option<Vec<_>>>()
            .map(|items| items.join(", ")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn a_skill_is_saved_only_when_it_reads_as_one() {
        let directory = temp_dir("skills_save");
        fs::create_dir_all(directory.join("notes")).unwrap();
        let good = "---\nname: notes\ndescription: Taking notes\nlicense: MIT\n---\nWrite.\n";
        fs::write(directory.join("notes").join(SKILL_FILE), good).unwrap();

        let skill = read(&directory, "notes").unwrap();
        assert!(skill.metadata.contains(&("license".into(), "MIT".into())));
        assert!(save(&directory, "notes", "no front matter").is_err());
        assert_eq!(
            read(&directory, "notes").unwrap().text,
            good,
            "a refused save keeps it"
        );
        assert!(read(&directory, "../notes").is_err());
        let edited = good.replace("Write.", "Write more.");
        assert_eq!(save(&directory, "notes", &edited).unwrap().text, edited);
    }
}
