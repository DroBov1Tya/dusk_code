//! A new session the agent proposes with `propose_session`: a card in the chat with the
//! session's title and brief, and two buttons. Nothing starts without the user's click,
//! in any mode. Start makes a new chat with the brief as its first message, linked back
//! to this one; Dismiss drops it. The agent hears which on its next turn, as a message
//! the core stores under the proposal's call; every client reads the card's state from
//! that message, or from `ThreadEvent::ProposalSettled` while the chat is open.

use serde::{Deserialize, Serialize};

use super::{links_in, thread_link};

/// The tool that proposes a session.
pub const PROPOSE_SESSION_TOOL: &str = "propose_session";
/// Proposals of one chat the user has not answered yet; one more is refused.
pub const MAX_PENDING_PROPOSALS: usize = 3;
/// The longest title, in characters.
pub const MAX_PROPOSAL_TITLE: usize = 80;
/// The longest brief, in characters.
pub const MAX_PROPOSAL_BRIEF: usize = 8000;

/// The arguments of `propose_session`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeSession {
    /// A short name for the session, as the chat list shows it.
    pub title: String,
    /// The first message of the new session: self-contained, since it does not see
    /// this chat. Say the goal, what is known, where to look and what done means.
    pub brief: String,
    /// The saved project it works in, by name or folder; this chat's when left out.
    pub project: Option<String>,
    /// A git branch to work on, named in the brief's first line.
    pub branch: Option<String>,
    /// The mode it starts in, such as PLAN; the current one when left out.
    pub mode: Option<String>,
}

/// The card, as the core checked it and stored it in the call's result.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// The proposal's own id, made by the core: unique, unlike a model's call id, which
    /// some models repeat or leave empty.
    pub id: String,
    pub title: String,
    pub brief: String,
    /// The project's folder the session opens in; this chat's when none.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// Where a proposal stands.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalState {
    Pending,
    /// The user started it: the new chat.
    Started {
        thread_id: String,
    },
    Dismissed,
}

/// The first line of the call's result, what the model reads before the card's data.
const SHOWN: &str = "The proposal is on the user's screen. Nothing starts unless they \
                     press Start; you hear what they chose on your next turn. Go on \
                     with the current task.";

impl Proposal {
    /// The call's result: a line for the model, then the card as JSON on the next.
    pub fn result(&self) -> String {
        // A card of plain strings always serializes.
        let json = serde_json::to_string(self).unwrap_or_default();
        format!("{SHOWN}\n{json}")
    }

    /// The card a stored or live result of `propose_session` holds; none for an error or
    /// anything else.
    pub fn read(result: &str) -> Option<Self> {
        let json = result.strip_prefix(SHOWN)?.strip_prefix('\n')?;
        let card: Self = serde_json::Deserializer::from_str(json)
            .into_iter()
            .next()?
            .ok()?;
        let fits = plain_id(&card.id)
            && !card.title.trim().is_empty()
            && card.title.chars().count() <= MAX_PROPOSAL_TITLE
            && card.brief.chars().count() <= MAX_PROPOSAL_BRIEF;
        fits.then_some(card)
    }

    /// The new session's first message: where it came from and its branch, then the
    /// brief.
    pub fn first_message(&self, origin: &str) -> String {
        let mut head = format!("Proposed in chat {}.", thread_link(origin));
        if let Some(branch) = &self.branch {
            head.push_str(&format!(" Work on branch `{branch}`."));
        }
        format!("{head}\n\n{}", self.brief)
    }
}

/// Whether `id` can be a proposal's id: a short word.
fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// What the agent reads of the outcome: the first words name the proposal.
const OUTCOME: &str = "[session proposal ";

/// The message the core stores in the proposing chat once the user answered.
pub fn outcome_note(id: &str, title: &str, state: &ProposalState) -> String {
    let what = match state {
        ProposalState::Started { thread_id } => format!(
            "The user started \"{title}\" as a new chat: {}.",
            thread_link(thread_id)
        ),
        ProposalState::Dismissed | ProposalState::Pending => {
            format!("The user dismissed \"{title}\".")
        }
    };
    format!("{OUTCOME}{id}] {what}")
}

/// The proposal and the outcome a stored note says; none for any other message.
pub fn read_outcome(text: &str) -> Option<(String, ProposalState)> {
    let rest = text.strip_prefix(OUTCOME)?;
    let (id, what) = rest.split_once("] ")?;
    if !plain_id(id) {
        return None;
    }
    let state = if what.starts_with("The user dismissed ") {
        ProposalState::Dismissed
    } else if what.starts_with("The user started ") {
        let thread_id = links_in(what).last()?;
        ProposalState::Started {
            thread_id: thread_id.to_owned(),
        }
    } else {
        return None;
    };
    Some((id.to_owned(), state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Proposal {
        Proposal {
            id: "0192-ab".into(),
            title: "Fix the gate".into(),
            brief: "The gate runs `uv run pytest`; make it `pytest`.".into(),
            project: None,
            branch: Some("gate-fix".into()),
            mode: None,
        }
    }

    #[test]
    fn a_card_reads_back_from_its_result_and_nothing_else_does() {
        let shown = card();
        assert_eq!(Proposal::read(&shown.result()), Some(shown.clone()));
        assert_eq!(Proposal::read("error: too many proposals"), None);
        assert_eq!(
            Proposal::read(&serde_json::to_string(&shown).unwrap()),
            None
        );
        let mut unnamed = shown.clone();
        unnamed.id = "a b".into();
        assert_eq!(Proposal::read(&unnamed.result()), None);
        let mut long = shown;
        long.title = "x".repeat(MAX_PROPOSAL_TITLE + 1);
        assert_eq!(Proposal::read(&long.result()), None);
    }

    #[test]
    fn an_outcome_reads_back_and_a_lookalike_does_not() {
        let started = ProposalState::Started {
            thread_id: "0192-ab".into(),
        };
        // A title cannot forge the chat it names: the last link is the core's.
        let title = "see dusk://thread/evil";
        for state in [started, ProposalState::Dismissed] {
            let note = outcome_note("0193-cd", title, &state);
            assert_eq!(read_outcome(&note), Some(("0193-cd".into(), state)));
        }
        for other in [
            "[session proposal ] The user dismissed \"x\".",
            "[session proposal a b] The user dismissed \"x\".",
            "[session proposal c] The agent started it.",
            "session proposal c] The user dismissed \"x\".",
        ] {
            assert_eq!(read_outcome(other), None, "{other}");
        }
    }

    #[test]
    fn the_first_message_links_back() {
        let text = card().first_message("origin");
        assert!(text.starts_with("Proposed in chat dusk://thread/origin. Work on branch"));
        assert!(text.ends_with("make it `pytest`."));
    }
}
