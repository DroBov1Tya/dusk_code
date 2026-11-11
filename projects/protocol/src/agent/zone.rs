//! The kind of work an agent belongs to, and what it does now.

/// The kind of work an agent does, from its role's name, so any client groups agents the
/// same way: a client draws zones, it does not decide them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Engineering,
    Security,
    Testing,
    Infrastructure,
    General,
}

impl Zone {
    /// The order zones are shown in.
    pub const ALL: [Zone; 5] = [
        Zone::Engineering,
        Zone::Security,
        Zone::Testing,
        Zone::Infrastructure,
        Zone::General,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Engineering => "Engineering",
            Self::Security => "Security",
            Self::Testing => "Testing and review",
            Self::Infrastructure => "Infrastructure",
            Self::General => "General",
        }
    }

    /// The first zone a word of the role belongs to; security goes first, so
    /// `cloud-security-specialist` is not infrastructure.
    pub fn of(role: &str) -> Zone {
        const WORDS: [(Zone, &[&str]); 4] = [
            (Zone::Security, &["security", "red-team", "appsec"]),
            (Zone::Testing, &["tester", "reviewer", "qa-"]),
            (
                Zone::Infrastructure,
                &[
                    "devops",
                    "sre",
                    "infrastructure",
                    "network",
                    "build-release",
                ],
            ),
            (Zone::Engineering, &["engineer", "architect", "docs"]),
        ];
        WORDS
            .iter()
            .find(|(_, words)| words.iter().any(|word| role.contains(word)))
            .map_or(Zone::General, |(zone, _)| *zone)
    }
}

/// What an agent is doing at this moment, as a word for any client to show; a tool
/// is told by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    Thinking,
    Writing,
    Searching,
    Reading,
    Planning,
    Running,
}

impl Doing {
    pub fn of_tool(name: &str) -> Doing {
        let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
        if has(&["search", "grep", "find", "glob"]) {
            Self::Searching
        } else if has(&["read", "fetch", "open", "load", "navigate", "snapshot"]) {
            Self::Reading
        } else if has(&["write", "edit", "plan", "task", "remember", "name"]) {
            Self::Planning
        } else {
            Self::Running
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Thinking => "thinking",
            Self::Writing => "writing",
            Self::Searching => "searching",
            Self::Reading => "reading",
            Self::Planning => "planning",
            Self::Running => "running",
        }
    }
}

#[cfg(test)]
mod zone_tests {
    use super::*;

    #[test]
    fn a_role_stands_in_the_zone_of_its_work() {
        assert_eq!(Zone::of("cloud-security-specialist"), Zone::Security);
        assert_eq!(Zone::of("qa-engineer"), Zone::Testing);
        assert_eq!(Zone::of("sre-engineer"), Zone::Infrastructure);
        assert_eq!(Zone::of("backend-engineer"), Zone::Engineering);
        assert_eq!(Zone::of("general"), Zone::General);
    }

    #[test]
    fn a_tool_is_told_by_its_name() {
        assert_eq!(Doing::of_tool("search_files"), Doing::Searching);
        assert_eq!(Doing::of_tool("read_file"), Doing::Reading);
        assert_eq!(Doing::of_tool("plan_edit"), Doing::Planning);
        assert_eq!(Doing::of_tool("run_command"), Doing::Running);
    }
}
