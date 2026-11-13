//! What the releases since some version brought, read from the changelog that is built
//! into the program: a client shows it once after an update, with no network.

const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// The entries of one or more releases, each a sentence or a few.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Notes {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub fixed: Vec<String>,
}

impl Notes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.fixed.is_empty()
    }
}

/// What came after `from` up to this build. With no `from`, or one the changelog does
/// not reach, only the newest release.
pub fn notes_since(from: Option<&str>) -> Notes {
    read(CHANGELOG, from)
}

fn read(changelog: &str, from: Option<&str>) -> Notes {
    let reaches = from.is_some_and(|from| {
        changelog
            .lines()
            .any(|line| release(line).is_some_and(|version| version == from))
    });
    let mut notes = Notes::default();
    let mut releases = 0;
    let mut open = false;
    let mut list: Option<&mut Vec<String>> = None;
    for line in changelog.lines() {
        if line.starts_with("## ") {
            list = None;
            // Work not released yet is not part of any update.
            let version = release(line);
            open = version.is_some();
            if !open {
                continue;
            }
            if (from.is_some() && version == from) || (!reaches && releases == 1) {
                break;
            }
            releases += 1;
        } else if let Some(kind) = line.strip_prefix("### ") {
            list = match (open, kind.trim()) {
                (false, _) => None,
                (true, "Added") => Some(&mut notes.added),
                (true, "Fixed" | "Security") => Some(&mut notes.fixed),
                (true, _) => Some(&mut notes.changed),
            };
        } else if let Some(list) = list.as_deref_mut() {
            if let Some(entry) = line.strip_prefix("- ") {
                list.push(entry.trim().to_owned());
            } else if let (Some(last), false) = (list.last_mut(), line.trim().is_empty()) {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    notes
}

/// The version a release heading names, as in `## 0.73.4 - 2026-11-13`.
fn release(line: &str) -> Option<&str> {
    let version = line.strip_prefix("## ")?.split(" - ").next()?.trim();
    version
        .starts_with(|first: char| first.is_ascii_digit())
        .then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\n## Unreleased\n\n### Added\n\n- not out yet\n\n\
        ## 0.3.0 - 2026-01-03\n\n### Added\n\n- a window that\n  spans two lines\n\n### Fixed\n\n- a crash\n\n\
        ## 0.2.0 - 2026-01-02\n\n### Changed\n\n- the rail\n\n## 0.1.0 - 2026-01-01\n\n### Added\n\n- the start\n";

    #[test]
    fn releases_after_the_last_seen_one_are_joined_and_unreleased_work_is_left_out() {
        let notes = read(LOG, Some("0.1.0"));
        assert_eq!(notes.added, ["a window that spans two lines"]);
        assert_eq!(notes.changed, ["the rail"]);
        assert_eq!(notes.fixed, ["a crash"]);
        // An unknown or missing version shows the newest release alone.
        assert_eq!(read(LOG, Some("0.0.1")), read(LOG, None));
        assert!(read(LOG, None).changed.is_empty());
        assert!(read(LOG, Some("0.3.0")).is_empty());
    }
}
