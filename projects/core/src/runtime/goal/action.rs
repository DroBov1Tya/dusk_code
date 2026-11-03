//! User-owned acceptance contracts. The executor may propose criteria and request
//! evidence review; it cannot approve, edit an approved contract, or complete it.

use anyhow::{Result, bail, ensure};

use crate::state::{Goal, GoalCheck, GoalCriterion, GoalStatus, State};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalAction {
    Show,
    Result(String),
    Set(String),
    Criteria(Vec<GoalCriterion>),
    Approve(String),
    Confirm {
        revision: String,
        criterion: String,
        result: String,
    },
    Pause,
    Resume,
    Clear,
}

impl GoalAction {
    pub fn from_command(argument: Option<&str>) -> Result<Self> {
        let text = argument.unwrap_or_default().trim();
        let (action, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let rest = rest.trim();
        Ok(match action {
            "" => Self::Show,
            "pause" if rest.is_empty() => Self::Pause,
            "resume" if rest.is_empty() => Self::Resume,
            "clear" if rest.is_empty() => Self::Clear,
            "done" => bail!("only runtime verification can complete a goal; /goal shows the gaps"),
            "approve" => {
                ensure!(
                    !rest.is_empty(),
                    "use /goal approve <revision> after reviewing /goal"
                );
                Self::Approve(rest.to_owned())
            }
            "confirm" => {
                let fields: Vec<_> = rest.split_whitespace().collect();
                ensure!(
                    fields.len() == 3,
                    "use /goal confirm <revision> <criterion> <result_id> after reviewing the saved result"
                );
                Self::Confirm {
                    revision: fields[0].into(),
                    criterion: fields[1].into(),
                    result: fields[2].into(),
                }
            }
            "criteria" => Self::Criteria(serde_json::from_str(rest)?),
            "result" => Self::Result(rest.to_owned()),
            _ => Self::Set(text.to_owned()),
        })
    }

    pub fn starts_work(&self) -> bool {
        matches!(self, Self::Set(_) | Self::Approve(_) | Self::Resume)
    }
}

/// Called only from the user's command path, never by the model's tool.
pub(crate) async fn apply(
    state: &State,
    thread_id: &str,
    action: GoalAction,
) -> Result<(Option<Goal>, String)> {
    let current = state.goal(thread_id).await?;
    let result = match action {
        GoalAction::Show => {
            let note = match &current {
                Some(goal) => format!(
                    "{}\nSaved results:\n{}",
                    describe(goal),
                    state.goal_result_index(thread_id).await?
                ),
                None => "no goal; /goal <text> creates a draft contract".into(),
            };
            return Ok((current, note));
        }
        GoalAction::Result(id) => {
            return Ok((current, state.inspect_goal_result(thread_id, &id).await?));
        }
        GoalAction::Set(text) => (
            Some(state.set_goal(thread_id, &text).await?),
            "draft goal; criteria need user approval",
        ),
        GoalAction::Criteria(criteria) => {
            let goal = current
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("no draft goal"))?;
            (
                Some(
                    state
                        .propose_goal(thread_id, &goal.revision, criteria)
                        .await?,
                ),
                "acceptance criteria proposed; review /goal",
            )
        }
        GoalAction::Approve(revision) => (
            Some(state.approve_goal(thread_id, &revision).await?),
            "acceptance contract approved",
        ),
        GoalAction::Confirm {
            revision,
            criterion,
            result,
        } => (
            Some(
                state
                    .confirm_goal(thread_id, &revision, &criterion, &result)
                    .await?,
            ),
            "saved result confirmed by user; runtime will check remaining criteria",
        ),
        GoalAction::Clear => {
            state.clear_goal(thread_id).await?;
            (None, "goal cleared")
        }
        GoalAction::Pause => (
            state
                .set_goal_status(thread_id, GoalStatus::Paused, None)
                .await?,
            "goal paused",
        ),
        GoalAction::Resume => (
            state
                .set_goal_status(thread_id, GoalStatus::Active, None)
                .await?,
            "goal resumed",
        ),
    };
    let note = match &result.0 {
        Some(goal) => format!("{}\n{}", result.1, describe(goal)),
        None => result.1.to_owned(),
    };
    Ok((result.0, note))
}

pub fn describe(goal: &Goal) -> String {
    let mut line = format!(
        "goal {} (round {}, contract {}, verification {}): {}\nrevision: {}",
        goal.status.name(),
        goal.rounds,
        if goal.approved {
            "approved"
        } else {
            "unapproved"
        },
        goal.verification,
        goal.text,
        goal.revision
    );
    for criterion in &goal.criteria {
        let method = match &criterion.check {
            GoalCheck::Command { command } => format!("exact command must exit 0: {command}"),
            GoalCheck::UserConfirmation => {
                "user must review and confirm a saved successful result".to_owned()
            }
        };
        line.push_str(&format!(
            "\n{}: {} | {method}",
            criterion.id, criterion.description
        ));
    }
    for evidence in &goal.evidence {
        line.push_str(&format!(
            "\n{} evidence: {}",
            evidence.criterion_id, evidence.result_id
        ));
    }
    for note in [&goal.note, &goal.verification_note].into_iter().flatten() {
        line.push_str(&format!("\n{note}"));
    }
    if !goal.approved {
        line.push_str(&format!("\nReview the criteria and their check methods, then /goal approve {}.\nEdit with /goal <text> or /goal criteria <JSON array>; legacy goals need a new contract.", goal.revision));
    } else {
        line.push_str("\nInspect evidence with /goal result <result_id>; semantic approval: /goal confirm <revision> <criterion> <result_id>.");
    }
    line
}

pub fn reminder(goal: &Goal, max_rounds: u32) -> String {
    if !goal.approved {
        return format!(
            "Draft goal; execution is blocked until user approval. Propose 1-20 mandatory acceptance criteria using the goal tool. Each criterion needs id, description and check: {{kind: command, command: exact shell command}} or {{kind: user_confirmation}}. Semantic requirements need user_confirmation. Do not perform the goal yet.\n{}",
            describe(goal)
        );
    }
    format!(
        "Standing goal (round {} of {max_rounds}). Work to the immutable approved criteria below. You cannot approve, edit, clear, pause or complete the goal. To finish, use goal show to find saved result IDs, then request_verification with the current revision and one result per criterion. Runtime reviews typed results; semantic criteria require explicit user confirmation. Successful commands prove only their approved exit check. Report missing user input and end the turn.\n{}",
        goal.rounds,
        describe(goal)
    )
}

pub fn kickoff(goal: &Goal, max_rounds: u32) -> String {
    if !goal.approved {
        return format!(
            "Propose acceptance criteria for the draft goal before doing its work: {}",
            goal.text
        );
    }
    if goal.rounds == 0 {
        format!("Goal: {}", goal.text)
    } else {
        format!(
            "Continue toward the goal (round {} of {max_rounds}): {}",
            goal.rounds, goal.text
        )
    }
}
