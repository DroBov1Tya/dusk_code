//! `propose_session`: the agent proposes a new session for work that is a task of its
//! own. The call checks the proposal and puts a card in the chat; it starts nothing. The
//! user's Start or Dismiss reaches the core as `ThreadRequest::Proposal`, which stores
//! the outcome for the agent's next turn under the proposal (`uid`), once. A proposal
//! is named by an id of its own, not by the model's call id, which some models repeat.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::{
    BoxFuture,
    catalog::history::plain_line,
    catalog::modes::Modes,
    state::State,
    tools::{Tool, ToolContext, schema, schema::parse_arguments},
};
use dusk_protocol::thread::{
    MAX_PENDING_PROPOSALS, MAX_PROPOSAL_BRIEF, MAX_PROPOSAL_TITLE, PROPOSE_SESSION_TOOL, Proposal,
    ProposalState, ProposeSession as Arguments, proposals, read_outcome,
};

/// Held while a proposal is answered, so two clicks cannot both find it waiting.
pub static ANSWERING: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The longest branch name taken.
const MAX_BRANCH: usize = 100;

pub struct ProposeSession {
    modes: PathBuf,
}

impl ProposeSession {
    /// `modes` is the folder of the modes a proposal may name.
    pub fn new(modes: PathBuf) -> Self {
        Self { modes }
    }
}

impl Tool for ProposeSession {
    /// It shows a card and keeps nothing of the project.
    fn writes_project(&self, _arguments: &Value) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        PROPOSE_SESSION_TOOL
    }

    // Only a chat has a user to start a session.
    fn root_only(&self) -> bool {
        true
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "Propose a new session (a chat of its own) for work that is a task of its own: \
             an unrelated follow-up you noticed, or a parallel piece the user could run \
             beside this one. Not for steps of the current task: do those here. The user \
             sees a card with Start and Dismiss; nothing starts unless they press Start, \
             and you hear what they chose on your next turn. The brief is the new \
             session's first message and it does not see this chat, so make it \
             self-contained: the goal, what is known, where to look, what done means. \
             At most {MAX_PENDING_PROPOSALS} proposals may wait for the user at once."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            ensure!(
                context.progress.is_some(),
                "there is no user to propose a session to in this thread"
            );
            let state = context
                .state
                .as_ref()
                .context("there is no chat to propose a session in")?;
            let proposal = check(arguments, state, &self.modes).await?;
            let waiting = pending(state, &context.thread_id).await?;
            ensure!(
                waiting < MAX_PENDING_PROPOSALS,
                "{waiting} proposals already wait for the user; wait for their answer \
                 before proposing more"
            );
            Ok(proposal.result())
        })
    }
}

/// The proposal as the card shows it: a plain title, a brief that fits, and the project,
/// branch and mode only when they are real ones.
async fn check(arguments: Arguments, state: &State, modes: &std::path::Path) -> Result<Proposal> {
    let title = plain_line(&arguments.title, MAX_PROPOSAL_TITLE + 1);
    ensure!(!title.is_empty(), "the title is empty");
    ensure!(
        title.chars().count() <= MAX_PROPOSAL_TITLE,
        "the title is at most {MAX_PROPOSAL_TITLE} characters"
    );
    let brief = arguments.brief.trim().to_owned();
    ensure!(!brief.is_empty(), "the brief is empty");
    ensure!(
        brief.chars().count() <= MAX_PROPOSAL_BRIEF,
        "the brief is at most {MAX_PROPOSAL_BRIEF} characters"
    );
    let project = match arguments.project.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(wanted) => {
            let projects = state.list_projects().await?;
            let found = projects.iter().find(|project| {
                project.name == wanted || project.dir == std::path::Path::new(wanted)
            });
            let Some(found) = found else {
                let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
                bail!(
                    "no saved project {wanted:?}; the saved ones are: {}",
                    names.join(", ")
                );
            };
            Some(found.dir.display().to_string())
        }
    };
    let branch = match arguments.branch.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(branch) => {
            let plain = branch
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c));
            ensure!(
                plain && branch.len() <= MAX_BRANCH && !branch.starts_with('-'),
                "a branch is a plain name such as fix/gate"
            );
            Some(branch.to_owned())
        }
    };
    let mode = match arguments.mode.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(mode) => {
            let known = Modes::new(modes).list();
            ensure!(
                known.iter().any(|known| known == mode),
                "no mode {mode:?}; the modes are: {}",
                known.join(", ")
            );
            Some(mode.to_owned())
        }
    };
    Ok(Proposal {
        id: uuid::Uuid::now_v7().to_string(),
        title,
        brief,
        project,
        branch,
        mode,
    })
}

/// Where the outcome of the proposal `id` of the chat `thread_id` is stored, once. The
/// chat is part of it, so a forked chat's copy of a card is answered on its own.
pub fn uid(thread_id: &str, id: &str) -> String {
    format!("proposal:{thread_id}:{id}")
}

/// The cards the chat `thread_id` holds, oldest first: only results of its
/// `propose_session` calls, never text another tool returned that reads as a card.
async fn cards(state: &State, thread_id: &str) -> Result<Vec<Proposal>> {
    Ok(proposals(&state.list_messages(thread_id).await?))
}

/// The proposal `id` in the chat `thread_id`, and where it stands.
pub async fn find(state: &State, thread_id: &str, id: &str) -> Result<(Proposal, ProposalState)> {
    let proposal = cards(state, thread_id)
        .await?
        .into_iter()
        .rfind(|card| card.id == id)
        .context("no such proposal in this chat")?;
    Ok((proposal, settled(state, thread_id, id).await?))
}

async fn settled(state: &State, thread_id: &str, id: &str) -> Result<ProposalState> {
    let stored = state.stored_under(&uid(thread_id, id)).await?;
    Ok(stored
        .and_then(|note| read_outcome(&note))
        .map_or(ProposalState::Pending, |(_, state)| state))
}

/// How many proposals of the chat wait for the user.
async fn pending(state: &State, thread_id: &str) -> Result<usize> {
    let mut waiting = 0;
    for card in cards(state, thread_id).await? {
        if settled(state, thread_id, &card.id).await? == ProposalState::Pending {
            waiting += 1;
        }
    }
    Ok(waiting)
}

/// Stores how the user answered, for the chat's next turn. False when it was answered
/// already, so a second click changes nothing.
pub async fn settle(
    state: &Arc<State>,
    thread_id: &str,
    id: &str,
    title: &str,
    outcome: &ProposalState,
) -> Result<bool> {
    let note = dusk_protocol::thread::outcome_note(id, title, outcome);
    // From the chat itself: never a command, and no answer to a question it waits on.
    state
        .add_pending_once(thread_id, &uid(thread_id, id), &note, Some(thread_id))
        .await
}

#[cfg(test)]
mod tests;
