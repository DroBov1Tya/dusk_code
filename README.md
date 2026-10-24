# dusk_code

Rust workspace for an async harness core. The core persists thread metadata and request receipts in SQLite and publishes in-memory events after successful writes. An in-process transport accepts user-message tasks without calling a model provider. The CLI initializes the state database, reports the Core API version, and exits.

```sh
cargo run -p dusk_code
cargo check --workspace
# Optional explicit database location:
cargo run -p dusk_code -- --state-db /tmp/dusk_code/state.db
```

```text
projects/
├── core/                 dusk_core library
│   └── src/
│       ├── lib.rs        public modules
│       ├── api.rs        Core API version and compatibility reexports
│       ├── lifecycle.rs  lifecycle states
│       ├── events/       aggregate event and one file per concrete payload
│       │   ├── mod.rs    reexports and Tokio broadcast types
│       │   ├── event.rs  Event enum
│       │   ├── thread_created.rs
│       │   ├── thread_updated.rs
│       │   ├── task_accepted.rs
│       │   ├── task_rejected.rs
│       │   └── lifecycle_changed.rs
│       ├── transport/    in-process request transport
│       │   ├── mod.rs    public reexports
│       │   ├── task.rs   Task contract
│       │   ├── execution.rs Execution and status contracts
│       │   └── handler.rs user-message receipt handler
│       ├── extensions.rs extension metadata and registry storage
│       ├── policy.rs     async authorization contract
│       ├── persistence.rs async snapshot contract
│       ├── state/        SQLite metadata, request receipts, and embedded migrations
│       └── supervision.rs cancellation and task ownership types
└── cli/                  dusk_code binary
    └── src/
        ├── main.rs       Tokio entry point
        ├── app.rs        build/run orchestration
        ├── args.rs       clap builder entry point
        ├── config.rs     logging setup
        └── constants.rs  application constants
```

Tokio provides the async runtime and concurrency primitives. Fallible contracts use `anyhow::Result`; policy and persistence futures are `Send`.

`state::State::open(path)` checks whether the database file exists before connecting, creates missing parent directories, and applies embedded SQLx migrations. The default CLI path is `~/Library/Application Support/dusk_code/state.db` on macOS, `%LOCALAPPDATA%/dusk_code/state.db` on Windows, and `$XDG_DATA_HOME/dusk_code/state.db` (or `~/.local/share/dusk_code/state.db`) elsewhere. `--state-db PATH` overrides it.

The `threads` table stores a caller-assigned stable `id`, `source`, core-assigned `created_at` and `updated_at` Unix timestamps in milliseconds, `tokens_used`, `compaction_count`, `sandbox_policy`, `approval_mode`, optional `git_branch`, `git_commit_sha`, and `git_origin_url`, and `title` (which may be empty). Source and sandbox policy are non-empty caller-defined strings. `ApprovalMode::OnRequest` (`on_request`) permits asking the user for approval; `ApprovalMode::Never` (`never`) disables approval requests. The sandbox policy is stored separately. Token and compaction counts range from zero to `i64::MAX`.

Each application table has one standalone migration file containing its complete schema and relevant indexes. `state/migrations/0001_threads.sql` creates `threads`; `0002_event_logs.sql` creates `event_logs` and its thread index. SQLx also maintains its own migration bookkeeping table.

Use `State::create_thread`, `update_thread`, and `load_thread` with `ThreadInput`. Creation rejects duplicate IDs. Updates preserve the ID and creation timestamp, return `None` for missing IDs, and keep update timestamps non-decreasing. `State::subscribe()` receives `ThreadCreated` and `ThreadUpdated` snapshots only after successful database writes. Writes through one state instance emit events in write order. A write with no listeners still succeeds.

`Task`, `Event`, and `Execution` are public core contracts. For an existing thread, pass a caller-assigned stable request UID and message content to the transport:

```rust,ignore
use dusk_core::{Task, transport::Transport};

let execution = Transport::new(&state)
    .handle(Task::UserMessage {
        request_uid: "request-001".into(),
        thread_id: "thread-001".into(),
        content: "User input".into(),
    })
    .await?;
assert!(!execution.is_complete());
```

The handler commits an `event_logs` receipt containing only `request_uid`, the attempted `thread_id`, `received_at` (Unix milliseconds), `status` (`accepted` or `rejected`), and optional `error_code`. Content is consumed in memory, never bound to a database query or included in the event. The caller must retain content for future processing. An accepted receipt means the core received a valid request; it remains incomplete because execution is not implemented. Rejected receipts have a fixed, non-sensitive code: `empty_message`, `empty_thread`, or `unknown_thread`. Rejection is terminal (`is_complete()` returns `true`) without generating model output. A rejected receipt is returned as `Ok(Execution)`; callers inspect `status` and `error_code`.

After the commit, subscribers receive `Event::TaskAccepted(TaskAccepted { execution })` or `Event::TaskRejected(TaskRejected { execution })`. Thread events wrap their metadata in `ThreadCreated { thread }` or `ThreadUpdated { thread }`; `LifecycleChanged { previous, current }` is a separate payload as well. Each payload has its own file under `events/`, and `Event` aggregates them. `Task` and `Execution` live under `transport/`; root imports and existing `api` imports remain available. Acceptance checks thread existence in the same database statement that inserts the receipt; `event_logs.thread_id` has no foreign key so unknown attempted threads can be recorded. Repeating a UID on the same thread returns its original receipt without another event, including after reopening the database. Neither accepted nor rejected rows are overwritten by retries. Content is not compared on retries; the caller must reuse a UID only for the same logical request. Reusing a UID for another thread fails and preserves the original receipt. Empty UIDs and unavailable storage return an error and cannot reliably leave a receipt. `State::load_execution(uid)` retrieves a durable receipt or returns `None`; a missing row alone therefore cannot distinguish an undelivered request from one that could not be logged. There is no core `not_received` status. `CoreEvent` remains a compatibility alias for `Event`.

## Limitations

This is an initial core slice. Scheduling, model execution, completed/failed model execution states, lifecycle transitions, general event routing, extension loading and compatibility negotiation, policy enforcement, snapshot persistence, and supervised shutdown are not implemented. `event_logs` stores request receipt metadata; it is not a replayable event journal. The event transport is in-process Tokio broadcast with a 256-event buffer, no replay, and no durable delivery. Receivers must handle lag; cancellation during a database operation can leave a committed write without a delivered event. Separate state instances have separate event channels. The CLI has no thread editing or chat interface yet.
