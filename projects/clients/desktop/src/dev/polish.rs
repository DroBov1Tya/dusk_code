//! `fake=chat`: a long chat with everything a transcript draws (folded turns, steps of
//! each state, markdown with code and a table, a picture, a notice, an error), tasks for
//! the plan strip and the Todo, pins and a context reading. `fake=chat-open` unfolds the
//! first turn and one of its steps. Made-up data only.

use dusk_protocol::{
    system::now_ms,
    thread::{Pin, PlanCounts, Task, Tool, ToolState},
};
use gpui_kit::*;

use crate::{
    pages::chat::{
        messages::STEP,
        transcript::{Item, Transcript},
    },
    workspace::{Center, Workspace},
};

const ANSWER: &str = "Entries now expire. The change is in three places:\n\n\
```bash\ncargo test -p cache --all-features -- --nocapture expiry::sweep_removes_entries_nobody_reads --test-threads=1 && cargo clippy -p cache --all-targets\n```\n\n\
## What changed\n\n\
1. `Entry` carries a `deadline`, set from the cache's `ttl` when the value is stored.\n\
2. `get()` compares the deadline with the clock and removes an entry that is past it.\n\
3. A sweep runs every 30 s for entries nobody reads.\n\n\
```rust\npub fn get(&mut self, key: &str) -> Option<&str> {\n    let expired = self.entries.get(key)?.deadline <= Instant::now();\n    if expired {\n        self.entries.remove(key);\n        return None;\n    }\n    self.entries.get(key).map(|entry| entry.value.as_str())\n}\n```\n\n\
| Check | Result | Time |\n|---|---|---|\n| unit tests | 14 passed | 0.8 s |\n| clippy | clean | 3.1 s |\n| expiry property test | 256 cases | 1.4 s |\n\n\
The sweep is **off** in tests, so they stay deterministic; see the [Instant docs](https://doc.rust-lang.org/std/time/struct.Instant.html) for why the clock is monotonic.";

const SECOND: &str = "Yes: `ttl` is read once, when the cache is built. A cache that is \
already running keeps its old value until it is rebuilt.\n\n\
- Changing it live needs a lock around the sweep.\n\
\x20 - The sweep takes it for the whole pass.\n\
\x20   - A read waits at most one pass.\n\
\x20 - `get()` takes it for one entry.\n\
- Entries stored earlier keep the deadline they were given.\n\n\
1. Build the cache with the new `ttl`.\n\
2. Move the entries over.\n\
\x20  1. Live ones first.\n\
\x20  2. Then the rest.\n\
10. Drop the old cache.\n\n\
> A shorter `ttl` never shortens the life of what is already stored.";

/// `fake=mcp`: five servers, one of each state, in the place of the config's.
pub fn mcp(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    use dusk_protocol::config::{McpInfo, McpState};
    let server = |name: &str, target: &str, remote: bool, state: McpState| McpInfo {
        name: name.into(),
        target: target.into(),
        remote,
        state,
        tools: Vec::new(),
        icon: None,
        added_from: None,
    };
    let ready = |tools: &[&str]| McpState::Ready(tools.iter().map(|tool| (*tool).into()).collect());
    if let Some(settings) = ws.settings.as_mut() {
        settings.mcp = vec![
            server(
                "github",
                "https://mcp.example.com/github",
                true,
                ready(&["a", "b", "c"]),
            ),
            server(
                "postgres",
                "npx -y @example/postgres-mcp --read-only",
                false,
                ready(&["query"]),
            ),
            server(
                "browser",
                "npx -y @example/browser-mcp",
                false,
                McpState::Starting,
            ),
            server(
                "sentry",
                "https://mcp.example.com/sentry",
                true,
                McpState::Off,
            ),
            server(
                "figma",
                "",
                true,
                McpState::Failed("the server answered 401: sign in again".into()),
            ),
        ];
    }
    cx.notify();
}

pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    if value == "chat-skill" {
        return skill(ws, cx);
    }
    let tool = |id: &str, name: &str, detail: &str, state: ToolState| {
        Item::Tool(Tool {
            call_id: id.into(),
            name: name.into(),
            detail: detail.into(),
            arguments: format!("{{\"path\": \"{detail}\"}}"),
            output: "running 14 tests\ntest ttl::expires_after_deadline ... ok\n\
                     test ttl::keeps_live_entries ... ok\ntest result: ok. 14 passed; 0 failed"
                .into(),
            state,
        })
    };
    let said = |text: &str, reasoning: &str| Item::Assistant {
        text: text.into(),
        reasoning: reasoning.into(),
    };
    let mut chat = Transcript::default();
    chat.extend(
        vec![
            Item::User {
                text: "Add expiry to the cache: an entry should be gone after `ttl`, and `get()` \
                   must never return a stale value."
                    .into(),
                sequence: Some(1),
                answered: Vec::new(),
            },
            said(
                "",
                "The cache keeps values in a map; a deadline per entry is the smallest change.",
            ),
            tool("a1", "read_file", "src/lib.rs", ToolState::Done),
            tool("a2", "search_files", "ttl", ToolState::Done),
            tool("a3", "edit_file", "src/lib.rs", ToolState::Done),
            tool(
                "a4",
                "run_command",
                "cargo test -p cache",
                ToolState::Failed,
            ),
            tool("a5", "edit_file", "src/lib.rs", ToolState::Done),
            tool("a6", "run_command", "cargo test -p cache", ToolState::Done),
            said(ANSWER, ""),
            Item::User {
                text: "Can `ttl` be changed while the cache runs?".into(),
                sequence: Some(2),
                answered: Vec::new(),
            },
            tool("b1", "read_file", "src/config.rs", ToolState::Done),
            said(SECOND, ""),
            Item::Image {
                card: dusk_protocol::tools::Card {
                    id: "0000-face".into(),
                    title: Some("Entry lifetime".into()),
                    description: Some(
                        "Stored at t0, readable until `t0 + ttl`, swept after.".into(),
                    ),
                    ..Default::default()
                },
                call_id: None,
                aside: false,
            },
            Item::Notice("The context was compacted: 41.2k tokens summed up into 3.8k.".into()),
            Item::Error(
                "The provider closed the stream: 529 overloaded. Send again to go on.".into(),
            ),
        ],
        0,
    );
    let now = now_ms();
    chat.activity_boundaries = vec![0, 9, chat.items.len()];
    chat.boundary_times = vec![now - 600_000, now - 466_000, now - 430_000];
    chat.boundary_spend = vec![None, Some((48_200, 212_000)), Some((6_100, 31_000))];
    chat.tokens = Some((58_400, 128_000));
    chat.cost = Some("$0.24".into());
    for (at, id) in [(0, "m1"), (8, "m2"), (9, "m3"), (11, "m4")] {
        chat.set_message_id(at, id.into());
    }
    chat.pins = pins(now);
    chat.tasks = tasks(now);
    // The picture card reads its file from the pictures folder of the throwaway profile.
    let folder = ws.status.artifacts_dir.join("0000-face");
    if std::fs::create_dir_all(&folder).is_ok() {
        let _ = std::fs::write(folder.join("image"), super::screenshot::demo_png());
    }
    let thread_id = ws.shown.clone().unwrap_or_else(|| "shot".to_owned());
    if value == "chat-open" {
        use crate::pages::chat::messages::ACTIVITY;
        ws.unfolded.insert((thread_id.clone(), 1 | ACTIVITY));
        ws.unfolded.insert((thread_id.clone(), 5 | STEP));
    }
    ws.dock.counts = Some((
        thread_id.clone(),
        PlanCounts {
            open: 2,
            claimed: 0,
            running: 1,
            blocked: 1,
            review: 1,
            done: 3,
            failed: 0,
        },
    ));
    ws.chats.insert(thread_id.clone(), chat);
    ws.shown = Some(thread_id);
    ws.center = Center::Chat;
    cx.notify();
}

/// `fake=chat-skill`: one turn whose step changed a skill, the row open on its diff, and a
/// second change already undone.
fn skill(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    use crate::pages::chat::messages::{ACTIVITY, SkillRow};
    use dusk_protocol::catalog::{DiffKind, DiffLine, SkillChange, SkillDiff, SkillSaveAction};
    let step = |id: &str| {
        Item::Tool(Tool {
            call_id: id.into(),
            name: "skill_evolve".into(),
            detail: "release".into(),
            arguments: "{}".into(),
            output: String::new(),
            state: ToolState::Done,
        })
    };
    let mut chat = Transcript::default();
    chat.extend(
        vec![
            Item::User {
                text: "Cut the release.".into(),
                sequence: Some(1),
                answered: Vec::new(),
            },
            step("s1"),
            step("s2"),
            Item::Assistant {
                text: "Released 1.4.0.".into(),
                reasoning: String::new(),
            },
        ],
        0,
    );
    chat.activity_boundaries = vec![0, 4];
    chat.boundary_times = vec![now_ms() - 90_000, now_ms()];
    chat.boundary_spend = vec![None, None];
    let change = |id: i64, call: &str, action, reason: &str, undone: bool| SkillChange {
        id,
        skill: "release".into(),
        call_id: call.into(),
        action,
        reason: reason.into(),
        changed_at_ms: now_ms(),
        undone,
        can_undo: !undone,
    };
    let line = |kind, text: &str| DiffLine {
        kind,
        text: text.into(),
    };
    let first_change = change(
        1,
        "s1",
        SkillSaveAction::Updated,
        "tag after the changelog is committed, not before",
        false,
    );
    let first = SkillRow {
        diff: Some(SkillDiff {
            change: first_change.clone(),
            lines: vec![
                line(DiffKind::Same, "## Release"),
                line(DiffKind::Same, ""),
                line(DiffKind::Removed, "1. Tag the commit."),
                line(DiffKind::Removed, "2. Update CHANGELOG.md."),
                line(DiffKind::Added, "1. Update CHANGELOG.md and commit it."),
                line(DiffKind::Added, "2. Tag that commit."),
                line(DiffKind::Same, "3. Push the tag."),
            ],
            cut: false,
        }),
        ..SkillRow::default()
    };
    let second = change(
        2,
        "s2",
        SkillSaveAction::Updated,
        "run the dry run first",
        true,
    );
    chat.skills_loaded(vec![first_change, second]);
    chat.skill_rows.insert(1, first);
    let thread_id = ws.shown.clone().unwrap_or_else(|| "shot".to_owned());
    ws.unfolded.insert((thread_id.clone(), 1 | ACTIVITY));
    ws.unfolded.insert((
        thread_id.clone(),
        (1usize & ((1 << 43) - 1)) | crate::pages::chat::messages::SKILL,
    ));
    ws.chats.insert(thread_id.clone(), chat);
    ws.shown = Some(thread_id);
    ws.center = Center::Chat;
    cx.notify();
}

fn pins(now: i64) -> Vec<Pin> {
    let pin = |id: &str, sequence: u64, role: &str, excerpt: &str, compacted: bool| {
        serde_json::from_value::<Pin>(serde_json::json!({
            "message_id": id, "sequence": sequence, "role": role, "excerpt": excerpt,
            "pinned_at": now - 60_000, "compacted": compacted,
        }))
        .expect("a made-up pin is within caps")
    };
    vec![
        pin(
            "m1",
            1,
            "User",
            "Add expiry to the cache: an entry should be gone after ttl, and get() must never return a stale value.",
            true,
        ),
        pin(
            "m2",
            2,
            "Assistant",
            "Entries now expire. The change is in three places",
            false,
        ),
        pin(
            "m4",
            4,
            "Assistant",
            "Yes: ttl is read once, when the cache is built.",
            false,
        ),
    ]
}

fn tasks(now: i64) -> Vec<Task> {
    let task = |n: u32, text: &str, status: &str, owner: Option<&str>, confirm: bool| {
        let mut json = serde_json::json!({
            "task_id": format!("t{n}"), "description": text, "done": status == "done",
            "created_at": now - 500_000, "updated_at": now - 60_000, "items": [],
            "status": status, "phase": "expiry",
        });
        if let Some(name) = owner {
            json["owner"] =
                serde_json::json!({ "thread_id": format!("0000000{n}-fake"), "name": name });
        }
        if confirm {
            json["gate"] = serde_json::json!({ "kind": "user_confirmation" });
        }
        // The task in work has items, so the Todo's card shows its line of them.
        if status == "running" {
            let item = |id: u32, text: &str, done: bool, started: bool| {
                serde_json::json!({
                    "item_id": format!("i{id}"), "description": text, "done": done,
                    "created_at": now - 400_000, "updated_at": now - 60_000,
                    "started_at": started.then_some(now - 300_000),
                    "finished_at": done.then_some(now - 200_000),
                })
            };
            json["items"] = serde_json::json!([
                item(1, "Put the sweep behind a lock", true, true),
                item(2, "Read ttl through the lock in get()", false, true),
                item(3, "Test a change while a sweep runs", false, false),
            ]);
        }
        serde_json::from_value::<Task>(json).expect("a made-up task reads back")
    };
    vec![
        task(1, "Give Entry a deadline", "done", Some("Ember-79"), false),
        task(
            2,
            "Remove expired entries in get()",
            "done",
            Some("Ember-79"),
            false,
        ),
        task(
            3,
            "Property test for expiry",
            "done",
            Some("Fable-3a"),
            false,
        ),
        task(
            4,
            "Sweep entries nobody reads",
            "review",
            Some("Ember-79"),
            true,
        ),
        task(
            5,
            "Make ttl changeable while the cache runs, behind a lock around the sweep",
            "running",
            Some("Fable-3a"),
            false,
        ),
        task(
            6,
            "Document the expiry in the README",
            "blocked",
            None,
            false,
        ),
        task(7, "Benchmark get() with a full map", "open", None, false),
        task(8, "Release notes", "open", None, false),
    ]
}

/// `fake=retry|unreachable|busy-threads|limits-high|agent-tree`: states that need a live
/// provider, a running turn or a broken config to come by.
pub fn state(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    use dusk_protocol::{
        model::{PlanWindow, ProviderErrorKind, RateLimits},
        thread::{ThreadEvent, Turn, TurnStatus, TurnWait},
    };
    let turn = |thread: &str, waiting| Turn {
        turn_id: "polish".into(),
        thread_id: thread.into(),
        last_sequence: 1,
        response_message_id: None,
        status: TurnStatus::Running,
        started_at: now_ms() - 140_000,
        finished_at: None,
        error_message: None,
        waiting,
        steps: 3,
        tokens: 18_420,
        cost_micro_usd: 0,
    };
    let shown = ws.shown.clone().unwrap_or_default();
    match value {
        // A model call being tried again: the row counts down to the next attempt.
        "retry" => {
            if let Some(chat) = ws.chats.get_mut(&shown) {
                chat.apply(ThreadEvent::TurnStarted(turn(&shown, None)));
                chat.apply(ThreadEvent::Retrying {
                    attempt: 2,
                    error: "529 overloaded".into(),
                    kind: ProviderErrorKind::Overloaded,
                    wait_ms: 8_000,
                    max_attempts: Some(6),
                });
            }
        }
        // The turn waits for a provider that does not answer.
        "unreachable" => {
            if let Some(chat) = ws.chats.get_mut(&shown) {
                chat.apply(ThreadEvent::TurnStarted(turn(&shown, None)));
                let now = now_ms();
                let waiting = TurnWait::provider(ProviderErrorKind::Unreachable, now, now + 45_000);
                chat.apply(ThreadEvent::TurnWaiting(turn(&shown, waiting)));
            }
        }
        // Two other chats of the list at work, so their rows show it.
        "busy-threads" => {
            let others: Vec<String> = ws
                .threads
                .iter()
                .map(|thread| thread.id.clone())
                .filter(|id| *id != shown)
                .take(3)
                .skip(1)
                .collect();
            for id in others {
                let mut chat = Transcript::default();
                chat.working = true;
                ws.chats.insert(id, chat);
            }
        }
        // The plan's windows near their end: the rings turn amber and red and breathe.
        "limits-high" => {
            ws.status.chatgpt_model = true;
            let window = |minutes, used_percent, resets_in| PlanWindow {
                minutes: Some(minutes),
                used_percent,
                resets_in: Some(resets_in),
            };
            ws.status.limits = Some(RateLimits {
                requests: None,
                tokens: None,
                plan: vec![window(300, 88, 2_400), window(10_080, 97, 86_400)],
            });
            if let Some(chat) = ws.chats.get_mut(&shown) {
                chat.tokens = Some((113_000, 128_000));
            }
        }
        // A tree of agents with levels, links both ways, and what could not be read.
        "agent-tree" => agent_tree(ws),
        kind if kind.starts_with("activity-") => {
            activity(ws, &shown, &kind["activity-".len()..], turn(&shown, None));
        }
        other => eprintln!("no such state: {other}"),
    }
    cx.notify();
}

/// `fake=activity-thinking|stalled|writing|planning|steps|agents|waiting|compacting`: a
/// turn of the shown chat in each kind of work the working row tells apart (`fake=retry`
/// and `fake=unreachable` are the held one).
fn activity(ws: &mut Workspace, shown: &str, kind: &str, turn: dusk_protocol::thread::Turn) {
    use dusk_protocol::{
        model::{ModelEvent, ToolCall},
        thread::{LiveTurn, ThreadEvent},
        tools::{ApprovalRequest, Request, ToolResult},
    };
    let Some(chat) = ws.chats.get_mut(shown) else {
        return;
    };
    let call = |id: &str, name: &str, arguments: serde_json::Value| {
        ThreadEvent::ToolStarted(ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        })
    };
    let done = |id: &str, failed: bool| {
        let mut result = ToolResult::text(if failed { "exit 1" } else { "ok" }.into());
        result.is_error = failed;
        ThreadEvent::ToolFinished {
            call_id: id.into(),
            result,
        }
    };
    let started = ThreadEvent::TurnStarted(turn);
    // One kind after another in the same turn, so the row's words swap as they do live.
    if !chat.working {
        chat.apply(started.clone());
    }
    let steps = chat.items.len();
    let events = match kind {
        "thinking" => vec![ThreadEvent::Model(ModelEvent::Reasoning(
            "Let me look.".into(),
        ))],
        "stalled" => Vec::new(),
        "writing" => vec![ThreadEvent::Model(ModelEvent::Text(
            "The list measures a row again whenever its text grows, and a reader inside \
             that row is sent back to its top. Keeping the scroll anchor through the measure"
                .into(),
        ))],
        "planning" => vec![
            ThreadEvent::Tasks(tasks(now_ms())),
            call("plan", "tasks", serde_json::json!({ "action": "add" })),
        ],
        "steps" => vec![
            call(
                "read",
                "read_file",
                serde_json::json!({ "path": "src/cache.rs" }),
            ),
            done("read", false),
            call(
                "test",
                "run_command",
                serde_json::json!({ "command": "cargo test -p cache" }),
            ),
            done("test", true),
            call(
                "find",
                "search_files",
                serde_json::json!({ "query": "deadline" }),
            ),
        ],
        "agents" => {
            let mut events = Vec::new();
            for agent in ["a", "b", "c"] {
                let id = format!("ask-{agent}");
                events.push(call(
                    &id,
                    "ask_agent",
                    serde_json::json!({ "task": "Review" }),
                ));
                events.push(ThreadEvent::SubagentStarted {
                    call_id: id,
                    thread_id: format!("agent-{agent}"),
                    agent: "general".into(),
                    task: "Review the change".into(),
                    effort: None,
                    model: None,
                    task_id: None,
                });
            }
            events.push(ThreadEvent::AgentsWorking(true));
            events.push(ThreadEvent::Subagent {
                thread_id: "agent-b".into(),
                event: Box::new(ThreadEvent::Idle),
            });
            events
        }
        "waiting" => vec![
            call(
                "publish",
                "run_command",
                serde_json::json!({ "command": "cargo publish" }),
            ),
            ThreadEvent::Approval(ApprovalRequest {
                id: 91,
                request: Request::Command {
                    command: "cargo publish -p cache".into(),
                },
                session: None,
                steps: Vec::new(),
            }),
        ],
        "compacting" => vec![ThreadEvent::CompactionStarted],
        other => {
            eprintln!("no such activity: {other}");
            Vec::new()
        }
    };
    for event in events {
        chat.apply(event);
    }
    match kind {
        // Nothing came for a long while: the turn's only event is forty seconds old.
        "stalled" => {
            chat.live = LiveTurn::default();
            chat.live.apply(&started, now_ms() - 40_000);
        }
        // The steps unfolded, so each state's mark is in sight.
        "steps" => {
            ws.unfolded.insert((
                shown.to_owned(),
                steps | crate::pages::chat::messages::ACTIVITY,
            ));
        }
        _ => {}
    }
}

/// `fake=queued`: two messages written while the shown chat works, and one the core holds
/// while it compacts the context.
pub fn queued(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    use crate::pages::chat::composer::Draft;
    let Some(shown) = ws.shown.clone() else {
        return;
    };
    for text in [
        "Also cover the empty list in the test",
        "Then rename the helper to load_all",
    ] {
        ws.queue(shown.clone(), text.into(), Draft::default(), window, cx);
    }
    ws.held_by_core(shown, "Keep the cached config here".into(), window, cx);
}

fn agent_tree(ws: &mut Workspace) {
    use dusk_protocol::catalog::{AgentNode, AgentTree, Problem};
    let node = |name: &str, about: &str, links: &[&str], children: Vec<AgentNode>| AgentNode {
        name: name.into(),
        description: about.into(),
        version: None,
        enabled: name != "docs",
        links: links.iter().map(|link| (*link).to_owned()).collect(),
        children,
        source: None,
    };
    let problem = |node: &str, reason: &str| Problem {
        node: node.into(),
        reason: reason.into(),
    };
    let Some(settings) = ws.settings.as_mut() else {
        eprintln!("fake=agent-tree: open a shelf first, so the settings are read");
        return;
    };
    settings.agents = AgentTree {
        root: node(
            "chat",
            "",
            &[],
            vec![
                node(
                    "general",
                    "Works like the chat in MAIN mode, with every tool the chat has.",
                    &[],
                    vec![],
                ),
                node(
                    "appsec-reviewer",
                    "Application security engineering and review: threat models, trust \
                     boundaries, injection, authorization and secrets handling.",
                    &[],
                    vec![],
                ),
                node(
                    "architect",
                    "System design, integration boundaries and structural trade-offs.",
                    &["code-reviewer", "appsec-reviewer", "qa-engineer"],
                    vec![node("planner", "Breaks a goal into tasks.", &[], vec![])],
                ),
                node(
                    "code-reviewer",
                    "Independent defect review with QA, performance and maintainability \
                     in mind; reports findings by severity.",
                    &["appsec-reviewer"],
                    vec![],
                ),
                node("docs", "Writes what changed.", &[], vec![]),
                node("qa-engineer", "", &[], vec![]),
            ],
        ),
        problems: vec![
            problem("release-manager", "agent.md has no description"),
            problem(
                "translator",
                "the front matter is not YAML: line 3, column 7",
            ),
        ],
        skipped: vec![problem(
            "architect",
            "asks data-engineer, which is not installed",
        )],
    };
}
