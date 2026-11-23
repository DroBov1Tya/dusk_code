//! A project's pull requests and CI, from whichever forge serves its remote.

use serde::{Deserialize, Serialize};

/// How a pull request's checks, or one check, stand.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckState {
    Pending,
    Passing,
    Failing,
}

/// Where one check stands, finer than `CheckState`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStage {
    Queued,
    Running,
    Passed,
    Skipped,
    Failed,
}

impl CheckStage {
    pub fn state(self) -> CheckState {
        match self {
            CheckStage::Queued | CheckStage::Running => CheckState::Pending,
            CheckStage::Passed | CheckStage::Skipped => CheckState::Passing,
            CheckStage::Failed => CheckState::Failing,
        }
    }
}

/// One check of a pull request; text plain and capped by the core.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    /// The workflow it runs in; empty when the forge names none.
    pub workflow: String,
    pub stage: CheckStage,
    /// Its page, https only; empty when the forge gave none that is safe.
    pub url: String,
    /// Unix seconds; `None` when not known.
    pub started: Option<i64>,
    pub finished: Option<i64>,
}

/// Where a pull request stands on the forge.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// A pull request's checks counted. `state` is the worst of them: failing over
/// running over passing; `None` when there are none.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checks {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub state: Option<CheckState>,
    /// The checks themselves: failed first, then running, queued, passed and skipped;
    /// capped, so `total` may count more.
    pub list: Vec<Check>,
}

/// One pull request as the repository status shows it; text plain and capped.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullStatus {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    /// The head branch.
    pub branch: String,
    /// The author's login; empty when the forge gave none.
    pub author: String,
    /// Unix seconds; `None` when not known.
    pub created: Option<i64>,
    pub updated: Option<i64>,
    pub added: u64,
    pub removed: u64,
    /// How many files it changes; `None` when not known.
    pub files: Option<u64>,
    /// The page on the forge, https only; empty when the forge gave none that is safe.
    pub url: String,
    /// The page of all its checks, https only; empty when the forge gave none that is safe.
    pub checks_url: String,
    pub checks: Checks,
}

/// A project's repository at a glance. The core keeps it fresh while a client shows it
/// and sends `Event::RepositoryStatus` when it changes.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryStatus {
    /// The name of the repository's top folder; empty outside a repository.
    pub repository: String,
    /// `None` on a detached head or outside a repository.
    pub branch: Option<String>,
    /// Lines added and removed in the work tree against the last commit.
    pub stat: crate::project::DiffStat,
    /// Why there is no repository: "Not a git repository". `None` when read.
    pub unavailable: Option<String>,
    pub forge: Forge,
}

/// Where the repository's pull requests come from.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Forge {
    /// No `origin`, or one whose address has no host the core can read.
    #[default]
    None,
    /// No plugin that is on serves the remote's host.
    Unserved {
        host: String,
        offer: Option<ForgeOffer>,
    },
    /// A plugin that is on serves the host, but the tool it uses is missing or not
    /// signed in.
    SignIn(ForgeSignIn),
    Served(Box<ForgeStatus>),
}

impl Forge {
    /// The answer of the plugin that serves the remote, when there is one.
    pub fn served(&self) -> Option<&ForgeStatus> {
        match self {
            Forge::Served(status) => Some(status),
            _ => None,
        }
    }
}

/// A plugin that would serve the host.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeOffer {
    /// The plugin's name, as the Workshop lists it.
    pub plugin: String,
    /// Installed but off: "Switch on"; else "Get" opens its Workshop page.
    pub installed: bool,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeSignIn {
    pub host: String,
    pub plugin: String,
    /// What the user can type to fix it, plain, one line, at most 120 characters:
    /// "gh auth login", or how to install gh. Drawn as text, never run.
    pub hint: String,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeStatus {
    /// The plugin that answered.
    pub plugin: String,
    /// What the plugin calls the forge, plain, one line, at most 40 characters.
    pub name: String,
    /// The repository as the remote names it, such as `owner/repo`.
    pub repository: String,
    /// Every pull request's page is this plus its number: a link in a chat that is
    /// `pull_prefix` + digits is one of this repository's pull requests. Empty until the
    /// plugin first answered.
    pub pull_prefix: String,
    /// The current branch's pull requests first, then the other open ones; capped.
    pub pulls: Vec<PullStatus>,
    /// Why there are no pull requests to show, plain, for a quiet note; `None` when read.
    pub unavailable: Option<String>,
}

/// The pull requests of a project's current branch, for an icon beside its chats.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullSummary {
    pub project_id: String,
    pub pulls: Vec<PullStatus>,
    /// The worst checks state of them all; `None` when none has checks.
    pub checks: Option<CheckState>,
}

/// A pull request of a chat: one a message in it links. Kept once merged or closed.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPull {
    /// As the forge names it, such as `owner/repo`.
    pub repository: String,
    pub number: u64,
    /// The pull request as the core last read it; `None` until the first read.
    pub pull: Option<PullStatus>,
}

/// The pull requests `text` links, each once, in the order first linked: links that are
/// `prefix` (see `pull_number`) and digits, not followed by a letter or digit.
pub fn pull_numbers(text: &str, prefix: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    if prefix.is_empty() {
        return numbers;
    }
    // ASCII lowering keeps every byte where it was, so an index in one is one in the other.
    let (lower, wanted) = (text.to_ascii_lowercase(), prefix.to_ascii_lowercase());
    let mut from = 0;
    while let Some(found) = lower[from..].find(&wanted) {
        let start = from + found;
        let after = start + prefix.len();
        let digits = text[after..].bytes().take_while(u8::is_ascii_digit).count();
        let end = after + digits;
        from = after;
        if text[end..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        if let Some(number) = pull_number(&text[start..end], prefix)
            && !numbers.contains(&number)
        {
            numbers.push(number);
        }
    }
    numbers
}

/// The number of the pull request `url` points to when it starts with `prefix`, the
/// pull link base the forge plugin gave (ends with `/`); `None` for any other link.
pub fn pull_number(url: &str, prefix: &str) -> Option<u64> {
    if !prefix.ends_with('/') {
        return None;
    }
    let head = url.as_bytes().get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix.as_bytes()) {
        return None;
    }
    let rest = url.get(prefix.len()..)?;
    let end = rest.find(['/', '#', '?']).unwrap_or(rest.len());
    let digits = &rest[..end];
    (digits.len() <= 12 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse::<u64>().ok())
        .flatten()
        .filter(|number| *number > 0)
}

#[cfg(test)]
mod tests {
    use super::{pull_number, pull_numbers};

    #[test]
    fn a_text_links_only_the_repository_own_pulls() {
        let prefix = "https://github.com/o/r/pull/";
        let text = "Opened https://github.com/o/r/pull/12. See (https://GITHUB.com/o/r/pull/7/files), \
            again https://github.com/o/r/pull/12 and [x](https://github.com/o/r/pull/9#a).\n\
            Not https://github.com/o/r/pull/3a, https://github.com/o/r/pull/0, \
            https://github.com/o/rr/pull/4, https://github.com/o/r/pull/, \
            https://github.com/o/r/pull/99999999999999999999 or https://github.com/x/r/pull/5. \
            é https://github.com/o/r/pull/8é";
        assert_eq!(pull_numbers(text, prefix), [12, 7, 9]);
        assert!(pull_numbers(text, "").is_empty());
        assert!(
            pull_numbers(
                "https://github.com/o/r/pull/12",
                "https://github.com/o/r/pull"
            )
            .is_empty()
        );
        assert_eq!(pull_numbers("https://github.com/o/r/pull/1", prefix), [1]);
    }

    #[test]
    fn only_a_link_under_the_prefix_has_a_number() {
        let prefix = "https://github.com/DroBov1Tya/dusk_code/pull/";
        let base = "https://github.com/DroBov1Tya/dusk_code/pull";
        assert_eq!(pull_number(&format!("{base}/13"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13/files"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13#top"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13?x=1"), prefix), Some(13));
        assert_eq!(
            pull_number("https://github.com/drobov1tya/DUSK_CODE/pull/7", prefix),
            Some(7)
        );
        for other in [
            "https://github.com/someone/else/pull/13".to_owned(),
            "https://github.com/DroBov1Tya/dusk_code_fork/pull/13".to_owned(),
            "https://github.com/DroBov1Tya/dusk/pull/13".to_owned(),
            "http://github.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            "https://github.com.evil.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            "https://gitlab.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            format!("{base}/"),
            format!("{base}/0"),
            format!("{base}/12a"),
            format!("{base}/99999999999999999999"),
            "https://github.com/DroBov1Tya/dusk_code/issues/13".to_owned(),
            "é".to_owned(),
        ] {
            assert_eq!(pull_number(&other, prefix), None, "{other}");
        }
        assert_eq!(pull_number(&format!("{base}/13"), ""), None);
        assert_eq!(pull_number(&format!("{base}/13"), base), None);
    }
}
