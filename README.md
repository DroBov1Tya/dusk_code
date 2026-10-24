# dusk_code

Rust workspace for an async harness core. The core persists thread metadata in SQLite and publishes in-memory events after successful writes. The CLI initializes the state database, reports the Core API version, and exits.

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
│       ├── api.rs        Core API version identity
│       ├── lifecycle.rs  lifecycle states
│       ├── events.rs     event contract and Tokio broadcast types
│       ├── extensions.rs extension metadata and registry storage
│       ├── policy.rs     async authorization contract
│       ├── persistence.rs async snapshot contract
│       ├── state/        SQLite thread metadata and embedded migrations
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

Migrations for the threads table live in `state/migrations/`; SQLx also maintains its own migration bookkeeping table. Migration `0001` remains immutable for existing database checksums. Forward migration `0002` renames legacy `from` to `source`, removes `to`, copies timestamps and token usage, and initializes compaction count to zero, title to an empty string, and Git fields to NULL. Legacy sandbox booleans map to `enabled`/`disabled`; exact legacy permission mode `never` maps to `never`, while all other values conservatively map to `on_request`. Each schema migration and its relevant indexes stay in one SQL file for the threads table.

Use `State::create_thread`, `update_thread`, and `load_thread` with `ThreadInput`. Creation rejects duplicate IDs. Updates preserve the ID and creation timestamp, return `None` for missing IDs, and keep update timestamps non-decreasing. `State::subscribe()` receives `ThreadCreated` and `ThreadUpdated` snapshots only after successful database writes. Writes through one state instance emit events in write order. A write with no listeners still succeeds.

## Limitations

This is an initial core slice. Lifecycle transitions, general event routing, extension loading and compatibility negotiation, policy enforcement, snapshot persistence, and supervised shutdown are not implemented. Only thread metadata is persisted; there is no event journal. The event transport is in-process Tokio broadcast with a 256-event buffer, no replay, and no durable delivery. Receivers must handle lag; cancellation during a database operation can leave a committed write without a delivered event. Separate state instances have separate event channels. The CLI has no thread editing interface yet.
