# dusk_code

An AI agent system in Rust. All logic lives in one core library, `dusk_core`; front ends (the CLI today, a TUI and GUI later) are thin clients over it.

```sh
cargo run -p dusk_code
cargo test --workspace
```

## Layout

```text
projects/
    core/src/                dusk_core
        lib.rs               module list
        api.rs               core API version
        paths.rs             every application path
        init.rs              first-start setup of those paths
        config.rs            config.yml: providers and session settings
        events.rs            live events published after database writes
        state/               SQLite records, one file per table
            state.rs         database handle: open, create, encrypt, subscribe
            threads.rs       sessions
            messages.rs      what was said, in order
            event_logs.rs    request receipts
            turns.rs         one user request and its answer
            jobs.rs          background work
            migrations/      one standalone schema file per table
        receivers/           entry points into the core
            user_receiver.rs   accepts a user message and queues a turn
            agent_receiver.rs  hands a queued turn to the agent, records its outcome
        runtime/             the agent
            agent_runtime.rs   turn -> model -> streamed answer -> turn outcome
            context.rs         stored messages -> model input (role and text only)
        providers/           models
            provider.rs      providers by name, model discovery, stream dispatch
            chat.rs          chat input and streamed output types
            openai.rs        OpenAI-compatible client and stream parser
            sse.rs           bounded Server-Sent Events decoder
    cli/src/                 dusk_code binary
        main.rs, app.rs      startup: paths, config, provider, model, thread
        chat.rs              line-based chat and stream rendering
```

## How a request flows

1. `UserReceiver::submit_message` validates the message and commits, in one transaction, a receipt in `event_logs`, the user message and a queued turn.
2. `AgentRuntime::run_turn` claims the turn through `AgentReceiver`, builds the model input from the thread's messages, streams the answer to the caller and records it on the turn as completed or failed.
3. Every successful write publishes an `Event` to `State::subscribe` receivers.

## Files

```text
~/.config/duskcode/
    config.yml               providers and session settings (mode 0600)
    session/state.db         threads, turns and messages (mode 0600)
```

On first start the core creates the directories (mode 0700), a config template without providers and an empty database; existing files are never modified. Add a provider and restart:

```yaml
providers:
  main:
    kind: openai_compatible
    base_url: 'https://api.example.com/v1'
    api_key: 'sk-...'

session:
  record: full      # user messages and the whole model output (default)
  # record: answer  # user messages and model answers only
```

The CLI uses the first provider and its first catalogue model and starts a new thread per run. `--state-db PATH` overrides the database location.

## Guarantees

- The model receives only role and text; IDs, timestamps, cwd and attachments stay in the core. Reasoning is stored but never sent back as context.
- A request UID is idempotent: a retry on the same thread returns the original receipt without a new event. Rejected requests get a fixed code (`empty_message`, `empty_thread`, `unknown_thread`) and no turn.
- Provider responses are untrusted: sizes and idle time are bounded, redirects are refused, and upstream text never reaches error messages.
- Events are in-process only: no replay, and slow receivers can miss events.

## Limitations

No system prompt, tools, context compaction, turn cancellation, session resume or retries yet. Schemas are standalone per table without upgrade migrations, so a schema change needs a fresh database.
