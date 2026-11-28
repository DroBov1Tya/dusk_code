//! The agent's way into the user's terminals: what it may type without a yes, and what
//! it may not reach at all.

use std::path::Path;

use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;
use crate::{
    runtime::ThreadEvent,
    testing::{temp_dir, tool_context_with},
    tools::{Approvals, Decision, Progress, ToolScope},
};
use dusk_protocol::client::Event;

/// A project folder with a scope on it, and the app's terminals with one open there. None
/// where this machine gives no pseudo-terminal, as in some sandboxes: then the test is
/// skipped.
struct Rig {
    terminals: Arc<Terminals>,
    scope: ToolScope,
    project: std::path::PathBuf,
    outside: std::path::PathBuf,
    _app: UnboundedReceiver<Event>,
}

fn rig(name: &str) -> Option<Rig> {
    let base = temp_dir(name);
    std::fs::create_dir_all(base.join("project")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::create_dir_all(base.join("workspace")).unwrap();
    let base = std::fs::canonicalize(base).unwrap();
    let project = base.join("project");
    let scope = ToolScope::new(&project, &base.join("workspace")).unwrap();
    let terminals = Arc::new(Terminals::new("/bin/sh".into()));
    let (queue, app) = unbounded_channel();
    terminals.set_app(Some((1, queue)));
    match terminals.open_in_app(&project) {
        Ok(_) => {}
        Err(error) if format!("{error:#}").contains("pseudo-terminal") => {
            eprintln!("skipped: {error:#}");
            return None;
        }
        Err(error) => panic!("{error:#}"),
    }
    Some(Rig {
        terminals,
        scope,
        project,
        outside: base.join("outside"),
        _app: app,
    })
}

impl Rig {
    fn id_in(&self, folder: &Path) -> String {
        let shown = self.terminals.shown().unwrap();
        shown
            .into_iter()
            .find(|shown| shown.cwd == folder)
            .unwrap()
            .id
    }

    /// A call as the chat makes it in `mode`, answered with `decision` when the user is
    /// asked: the result, and what they were asked.
    async fn call(
        &self,
        mode: &str,
        arguments: Value,
        decision: Decision,
    ) -> (anyhow::Result<String>, Vec<Request>) {
        let (events, mut heard) = unbounded_channel();
        let approvals = Arc::new(Approvals::default());
        let context = ToolContext {
            approvals: Some(Arc::clone(&approvals)),
            progress: Some(Progress {
                call_id: "c".into(),
                events,
            }),
            ..tool_context_with(&self.scope, &format!("approvals: {mode}\n"))
        };
        let tool = Terminal(Arc::clone(&self.terminals));
        let call = tool.call(arguments, context);
        let answer = async {
            let mut asked = Vec::new();
            while let Some(event) = heard.recv().await {
                if let ThreadEvent::Approval(request) = event {
                    asked.push(request.request.clone());
                    approvals.answer(request.id, decision);
                }
            }
            asked
        };
        let (result, asked) = tokio::join!(call, answer);
        (result, asked)
    }
}

fn typed(id: &str, text: &str) -> Value {
    json!({ "action": "write", "terminal": id, "text": text, "enter": true })
}

/// The terminal's newest output, once `wanted` shows in it.
async fn printed(terminals: &Terminals, id: &str, wanted: &str) -> String {
    for _ in 0..100 {
        let (_, lines) = terminals.read(id, 50).unwrap();
        let text = lines.join("\n");
        if text.contains(wanted) {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{wanted:?} never showed");
}

#[tokio::test]
async fn typing_is_asked_with_exactly_what_is_typed_even_in_sandbox_mode() {
    let Some(rig) = rig("terminal-tool-ask") else {
        return;
    };
    let id = rig.id_in(&rig.project);
    for mode in ["ask", "sandbox"] {
        let (result, asked) = rig
            .call(mode, typed(&id, "printf 'n%s\\n' o"), Decision::Deny)
            .await;
        assert!(result.is_err(), "{mode}");
        let [Request::Terminal { input, .. }] = asked.as_slice() else {
            panic!("{mode}: {asked:?}");
        };
        assert_eq!(input, "printf 'n%s\\n' o [Enter]");
    }
    // Denied twice: nothing was typed.
    let (result, _) = rig
        .call("ask", typed(&id, "printf 'y%s\\n' es"), Decision::Once)
        .await;
    let result = result.unwrap();
    assert!(result.contains("untrusted program output"), "{result}");
    let text = printed(&rig.terminals, &id, "yes").await;
    assert!(!text.contains("no"), "{text}");
}

#[tokio::test]
async fn full_access_types_in_the_project_but_asks_outside_it() {
    let Some(rig) = rig("terminal-tool-full") else {
        return;
    };
    let inside = rig.id_in(&rig.project);
    let (result, asked) = rig
        .call("full_access", typed(&inside, "echo in"), Decision::Deny)
        .await;
    assert!(result.is_ok(), "{result:?}");
    assert!(asked.is_empty(), "{asked:?}");

    // A terminal the user opened in another folder.
    let (queue, _events) = unbounded_channel();
    rig.terminals.set_app(Some((1, queue)));
    rig.terminals.open_in_app(&rig.outside).unwrap();
    let outside = rig.id_in(&rig.outside);
    let (result, asked) = rig
        .call("full_access", typed(&outside, "echo out"), Decision::Deny)
        .await;
    assert!(result.is_err());
    assert_eq!(asked.len(), 1, "{asked:?}");
    // Reading it is asked too, as a read outside the project.
    let read = json!({ "action": "read", "terminal": outside });
    let (result, asked) = rig.call("ask", read, Decision::Deny).await;
    assert!(result.is_err());
    assert!(matches!(
        asked.as_slice(),
        [Request::Path { write: false, .. }]
    ));
    rig.terminals.close_all();
}

#[tokio::test]
async fn a_bad_call_or_no_app_reaches_no_terminal() {
    let Some(rig) = rig("terminal-tool-refused") else {
        return;
    };
    let id = rig.id_in(&rig.project);
    for arguments in [
        json!({ "action": "write", "terminal": id, "text": "ls\nrm -rf x" }),
        json!({ "action": "write", "terminal": id, "text": "\u{1b}[A" }),
        json!({ "action": "write", "terminal": id }),
        json!({ "action": "write", "text": "ls" }),
        json!({ "action": "write", "terminal": "no-such-id", "text": "ls" }),
        json!({ "action": "open", "cwd": rig.outside.display().to_string() }),
        json!({ "action": "open", "cwd": "../outside" }),
        json!({ "action": "read" }),
    ] {
        let (result, asked) = rig
            .call("full_access", arguments.clone(), Decision::Once)
            .await;
        assert!(result.is_err(), "{arguments}: {result:?}");
        assert!(asked.is_empty(), "{arguments}");
    }
    // Without the app, the terminal client or a browser: nothing is open here.
    rig.terminals.set_app(None);
    let list = json!({ "action": "list" });
    let (result, _) = rig.call("full_access", list, Decision::Once).await;
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains("no terminal open here"), "{error}");
    rig.terminals.close_all();
}

#[test]
fn keys_go_first_and_enter_last() {
    let arguments: Arguments = serde_json::from_value(json!({
        "action": "write",
        "keys": ["ctrl_c"],
        "text": "npm run dev",
        "enter": true,
    }))
    .unwrap();
    let typed = Typed::of(&arguments).unwrap().unwrap();
    assert_eq!(typed.bytes, b"\x03npm run dev\r");
    assert_eq!(typed.shown, "[Ctrl-C] npm run dev [Enter]");
}
