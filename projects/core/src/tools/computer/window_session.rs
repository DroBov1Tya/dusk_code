//! Select one window explicitly; its lifetime is shorter than the running chat.

use std::sync::Arc;

use anyhow::{Context, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::{Desktop, Screen, native_window::NativeWindow, shared::blocking};
use crate::{
    BoxFuture,
    platform::Needs,
    tools::{
        schema,
        schema::parse_arguments,
        tool::{Tool, ToolContext, ToolKind},
    },
};

pub struct WindowSession(pub Arc<Screen>);

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Select,
    Release,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    /// Both IDs come from list. Titles are descriptive and never select a window.
    window_id: Option<u32>,
    pid: Option<i32>,
}

impl Tool for WindowSession {
    computer_tool!("window_session");

    fn description(&self, _: &ToolContext) -> String {
        "On macOS, list windows, select one by window_id and pid, or explicitly release it. \
         Selection pins the exact accessible window. While selected, screenshot with no \
         arguments and its regions capture only that window; independent click may AXPress \
         its controls while another app covers it. AXPress can bring the target app to \
         the foreground. System clicks, scroll, type_text and \
         press_keys are disabled. Selection and release use separate approval requests."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let scope = self.0.scope();
            let expected = scope.as_ref().map(|(id, _)| id.clone());
            match arguments.action {
                Action::List => {
                    ensure!(
                        arguments.window_id.is_none() && arguments.pid.is_none(),
                        "list takes no IDs"
                    );
                    context
                        .approve_screen("list app window names and IDs", false)
                        .await?;
                    blocking(|| {
                        let windows = Desktop.windows()?;
                        let rows: Vec<_> = windows.iter().take(64).map(|window| serde_json::json!({
                            "window_id": window.id, "pid": window.pid, "app": window.app, "title": window.title,
                            "width": window.bounds.width, "height": window.bounds.height,
                        })).collect();
                        Ok(serde_json::to_string(&serde_json::json!({"windows": rows, "truncated": windows.len() > 64}))?)
                    }).await
                }
                Action::Select => {
                    let id = arguments
                        .window_id
                        .context("select requires window_id from list")?;
                    let pid = arguments.pid.context("select requires pid from list")?;
                    let selected = blocking(move || NativeWindow::select(id, pid)).await?;
                    let label = selected.label.clone();
                    context.approve_window(Some(selected.token.clone()),
                        &format!("capture and activate controls only in {label}; AXPress can bring it to the foreground")).await?;
                    let check = selected.clone();
                    blocking(move || check.inspect()).await?;
                    self.0.select(selected, expected.as_deref())?;
                    Ok(format!(
                        "Selected {label}. Take screenshot {{}} before clicking. Shared keyboard, pointer and scrolling are disabled until an approved release."
                    ))
                }
                Action::Release => {
                    ensure!(
                        arguments.window_id.is_none() && arguments.pid.is_none(),
                        "release takes no IDs"
                    );
                    let (id, label) = scope.context("no window session is selected")?;
                    context
                        .approve_window(
                            None,
                            &format!(
                                "end confinement to {label}; desktop tools become available again"
                            ),
                        )
                        .await?;
                    self.0.release(&id)?;
                    Ok(
                        "Window session released; previous screenshot coordinates were cleared."
                            .into(),
                    )
                }
            }
        })
    }
}
