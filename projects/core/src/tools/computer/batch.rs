//! `computer_actions`: several screen actions in one call, so a known sequence costs one
//! round trip. Every step is checked and asked for before the first one runs, and the
//! batch is never asked for less than its steps would be one by one.

use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{Click, PressKeys, Screenshot, TypeText, screen::Screen, step::Step};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        ToolResult, schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

/// Steps in one call at most.
const MAX_STEPS: usize = 20;
/// Screenshots in one call at most, as many pictures as an MCP result may bring.
const MAX_PICTURES: usize = 4;

pub struct ComputerActions(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchArguments {
    /// The actions, run in order; the batch stops at the first that fails.
    #[schemars(length(min = 1, max = MAX_STEPS))]
    steps: Vec<StepArguments>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StepArguments {
    tool: StepTool,
    /// What that tool takes when called alone.
    arguments: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum StepTool {
    Click,
    TypeText,
    PressKeys,
    Screenshot,
}

impl StepTool {
    fn name(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::TypeText => "type_text",
            Self::PressKeys => "press_keys",
            Self::Screenshot => "screenshot",
        }
    }
}

/// The steps as the model gave them, bounded. Hooks, switches and the run all read
/// them through this one parse, so none sees other steps than the rest.
fn parse_steps(arguments: Value) -> Result<Vec<StepArguments>> {
    let BatchArguments { steps } = parse_arguments(arguments)?;
    ensure!(
        (1..=MAX_STEPS).contains(&steps.len()),
        "give 1 to {MAX_STEPS} steps, not {}",
        steps.len()
    );
    let pictures = steps
        .iter()
        .filter(|step| step.tool == StepTool::Screenshot)
        .count();
    ensure!(
        pictures <= MAX_PICTURES,
        "take at most {MAX_PICTURES} screenshots in one batch"
    );
    Ok(steps)
}

impl ComputerActions {
    /// Every step checked, in order, before anything is asked or done.
    fn steps(&self, arguments: Value) -> Result<Vec<(StepTool, Step)>> {
        let steps = parse_steps(arguments)?;
        steps
            .into_iter()
            .enumerate()
            .map(|(index, step)| {
                let arguments = Value::Object(step.arguments);
                let screen = &self.0;
                let checked = match step.tool {
                    StepTool::Click => Click::step(screen, arguments),
                    StepTool::TypeText => TypeText::step(screen, arguments),
                    StepTool::PressKeys => PressKeys::step(screen, arguments),
                    StepTool::Screenshot => Screenshot::step(screen, arguments),
                };
                let checked = checked.with_context(|| {
                    format!(
                        "step {} ({}) is wrong, so no step ran",
                        index + 1,
                        step.tool.name()
                    )
                })?;
                Ok((step.tool, checked))
            })
            .collect()
    }
}

/// Runs the asked-for steps in order and stops at the first failure. Every step reads x
/// and y in the picture the model saw before the batch, not in one taken meanwhile that
/// it has not seen yet; after the batch the newest picture counts again.
async fn run(screen: &Screen, steps: Vec<(StepTool, Step)>) -> ToolResult {
    let total = steps.len();
    let seen = screen.coordinates();
    let mut newest = seen;
    let mut result = ToolResult::text(String::new());
    let mut lines = Vec::with_capacity(total);
    for (index, (tool, step)) in steps.into_iter().enumerate() {
        let number = index + 1;
        screen.set_coordinates(seen);
        match step.run().await {
            Ok(done) => {
                if tool == StepTool::Screenshot {
                    newest = screen.coordinates();
                }
                lines.push(format!("{number}. {}", done.content));
                result.artifacts.extend(done.artifacts);
            }
            Err(error) => {
                lines.push(format!("{number}. failed: {error:#}"));
                if number < total {
                    lines.push(format!("steps {} to {total} did not run", number + 1));
                }
                result.is_error = true;
                break;
            }
        }
    }
    screen.set_coordinates(newest);
    result.content = lines.join("\n");
    result
}

impl Tool for ComputerActions {
    computer_tool!("computer_actions");

    fn description(&self, _: &ToolContext) -> String {
        format!(
            "Run up to {MAX_STEPS} of click, type_text, press_keys and screenshot in \
             order in one call, each with the arguments it takes alone. Use it when you know \
             the coordinates and need not look between the actions, such as click a field, \
             type, press enter, then a screenshot to check. Every x, y is in the screenshot \
             taken before the batch. It asks the user once for all the steps and stops at \
             the first that fails."
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<BatchArguments>()
    }

    fn calls_within(&self, arguments: &Value) -> Result<Vec<(String, Value)>> {
        let steps = parse_steps(arguments.clone())?;
        Ok(steps
            .into_iter()
            .map(|step| (step.tool.name().to_owned(), Value::Object(step.arguments)))
            .collect())
    }

    fn call<'a>(&'a self, _: Value, _: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async { anyhow::bail!("computer_actions returns its steps' results") })
    }

    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            let steps = self.steps(arguments)?;
            let requests = steps.iter().map(|(_, step)| step.request.clone()).collect();
            context.approve_computer(requests).await?;
            Ok(run(&self.0, steps).await)
        })
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
