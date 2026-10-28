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
        session.rs           starts a session: workspace folder, thread, MAIN.md
        state/               SQLite records, one file per table
            state.rs         database handle: open, create, encrypt, subscribe
            threads.rs       sessions
            messages.rs      what was said, in order
            event_logs.rs    request receipts
            turns.rs         agent runs: each answers every queued message
            jobs.rs          background work
            migrations/      one standalone schema file per table
        receivers/           entry points into the core
            user_receiver.rs   stores a user message; it waits in the queue
            agent_receiver.rs  reads a thread, starts and finishes turns
        runtime/             the agent
            session_runner.rs  live session: send, send now, cancel, events
            answer.rs          one model call, streamed into a caller-owned answer
            context.rs         stored messages -> model input and its token size
        providers/           models
            provider.rs      providers by name, model discovery, stream dispatch
            chat.rs          chat input and streamed output types
            openai.rs        OpenAI-compatible client and stream parser
            sse.rs           bounded Server-Sent Events decoder
    cli/src/                 dusk_code binary
        main.rs, app.rs      startup: paths, config, provider, model, session
        chat.rs              line-based chat: `text` queues, `!text` sends now, `!` stops
```

## How a session works

A client sends text with `SessionInput` and reads `SessionEvent`s from `SessionOutput`; `SessionRunner` does the rest.

1. A message is stored with a receipt. User messages newer than the last turn are the queue.
2. When the agent is free it starts one turn over the whole queue, checks the context size against the model window, sends the thread's messages and streams the answer.
3. `send` during a turn waits for the next one; `send_now` stops the turn, keeping the text written so far, and the next turn starts at once; `cancel` only stops. Messages sent during a turn are stored after its answer, so the thread reads in the order things happened.
4. A turn ends `completed`, `interrupted` or `failed`. A turn left running by a stopped process is marked interrupted on the next start.
5. Every successful write publishes an `Event` to `State::subscribe` receivers.

## Files

```text
~/.config/duskcode/
    config.yml               providers and session settings (mode 0600)
    MAIN.md                  system prompt of the main agent (mode 0600)
    skills/<name>/SKILL.md   skills
    agents/<name>.md         system prompts of subagent kinds
    session/state.db         threads, turns and messages (mode 0600)
~/Documents/duskcode/
    <thread id>/             session workspace: temporary and downloaded files (mode 0700)
```

On first start the core creates the directories (mode 0700), a config template without providers, a default MAIN.md and an empty database; existing files are never modified. Add a provider and restart:

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

The CLI uses the first provider and its first catalogue model and starts a new session per run. `--state-db PATH` overrides the database location.

## Guarantees

- MAIN.md is copied into a new thread as its first, `system` message; editing the file affects only chats started later. A missing or blank file means no system prompt.
- The model receives only role and text; IDs, timestamps, cwd and attachments stay in the core. Reasoning is stored but never sent back as context.
- Text is stored once, in `messages`; turns refer to messages and every request is rebuilt from them. Each message carries an estimated token count (UTF-8 bytes / 4); providers report usage per request only.
- At most one turn runs per thread, enforced by the database.
- A request UID is idempotent: a retry on the same thread returns the original receipt without a new event. Rejected requests get a fixed code (`empty_message`, `empty_thread`, `unknown_thread`) and are not queued.
- Provider responses are untrusted: sizes and idle time are bounded, redirects are refused, and upstream text never reaches error messages.
- Events are in-process only: no replay, and slow receivers can miss events.

## Limitations

Skills and agent prompts are not loaded yet. No tools, context compaction, session resume or retries yet. Messages sent during a turn live in memory until it ends. An interrupted turn keeps its text only; reasoning without text is not stored. Schemas are standalone per table without upgrade migrations, so a schema change needs a fresh database.
