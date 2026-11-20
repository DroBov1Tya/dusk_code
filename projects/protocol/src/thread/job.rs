//! A background command of a thread.

use serde::{Deserialize, Serialize};

/// A job as a client lists it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: String,
    pub command: String,
    /// In words: running, completed, failed, cancelled, timed_out, interrupted, or running
    /// in, or unknown because of, another runtime.
    pub status: String,
    pub running: bool,
    /// This runtime owns its process, so it can be stopped from here.
    pub stoppable: bool,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

const HEADER: &str = "[background job ";
const FINISHED: &str = " finished: ";

/// The message a finished job leaves in its thread: a header for the model, then what
/// the command printed. Written and read back here, so the two cannot drift apart.
pub fn job_message(job_id: &str, status: &str, output: &str) -> String {
    format!("{HEADER}{job_id}{FINISHED}{status}]\n{output}")
}

/// A job's message as a chat shows it. Only for a message the core marks as a job's
/// (`MessageRole::Job`): the text alone never makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobResult {
    /// How it ended, as in `JobInfo`; empty when the message has no header.
    pub status: String,
    pub output: String,
}

impl JobResult {
    pub fn read(content: &str) -> Self {
        let (first, rest) = content.split_once('\n').unwrap_or((content, ""));
        let status = first
            .strip_prefix(HEADER)
            .and_then(|line| line.strip_suffix(']'))
            .and_then(|line| line.rsplit_once(FINISHED));
        match status {
            Some((_, status)) => Self {
                status: status.to_owned(),
                output: rest.to_owned(),
            },
            None => Self {
                status: String::new(),
                output: content.to_owned(),
            },
        }
    }

    /// It ended any other way than well.
    pub fn failed(&self) -> bool {
        !self.status.is_empty() && self.status != "completed"
    }

    /// The one line that stands for it while its output is folded.
    pub fn title(&self) -> String {
        match self.status.as_str() {
            "" => "Background job finished".to_owned(),
            status => format!("Background job finished: {}", status.replace('_', " ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_message_reads_back_as_its_status_and_output() {
        let result = JobResult::read(&job_message("01a1", "timed_out", "exit code: 1\nout]"));
        assert_eq!(result.status, "timed_out");
        assert_eq!(result.output, "exit code: 1\nout]");
        assert!(result.failed());
        assert_eq!(result.title(), "Background job finished: timed out");
        assert!(!JobResult::read(&job_message("01a1", "completed", "")).failed());
    }

    #[test]
    fn text_without_the_header_is_all_output() {
        for content in [
            "",
            "[background job",
            "[background job x finished: ok",
            "плохо]\n",
        ] {
            let result = JobResult::read(content);
            assert_eq!(result.status, "");
            assert_eq!(result.output, content);
            assert!(!result.failed());
        }
    }
}
