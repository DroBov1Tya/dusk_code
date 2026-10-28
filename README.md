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
        api.rs               core version
        paths.rs             every application path
        init.rs              first-start setup of those paths
        config.rs            config.yml: providers, session settings, tool switches
        live_config.rs       the current config in memory, written back on change
        events.rs            live events published after database writes
        session.rs           starts a session: workspace folder, thread, MAIN.md
        skills.rs            the skill graph read from skills/
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
            inbox.rs           client commands while the agent works
            agent.rs           a turn, step by step: model -> tools -> model
            generation.rs      one model call, streamed into a caller-owned output
            context.rs         stored messages -> model input and its token size
        tools/               what the agent can call
            tool.rs          the Tool interface
            registry.rs      tools by name; failed calls become error results
            scope.rs         where tools may act, checked after following symlinks
            files.rs         read_file, list_dir, write_file, edit_file
            search.rs        search: plain-text search through a directory
            shell.rs         run_command: sh -c in the working directory
            load_skill.rs    load_skill: walks the skill graph
        providers/           models
            provider.rs      providers by name, model discovery, stream dispatch
            chat.rs          model input: messages by role, tool specs
            model_output.rs  model output: ModelEvent stream, ModelOutput assembly
            openai.rs        OpenAI-compatible client and stream parser
            inline_tags.rs   <think> and <tool_call> tags in text -> ModelEvents
            sse.rs           bounded Server-Sent Events decoder
    cli/src/                 dusk_code binary
        main.rs, app.rs      startup: paths, config, provider, model, session
        chat.rs              line-based chat: `text` queues, `!text` sends now, `!` stops
        commands.rs          `:tools`, `:skills`, `:on <name>`, `:off <name>`, `:reload`
```

## How a session works

A client sends text with `SessionInput` and reads `SessionEvent`s from `SessionOutput`; `SessionRunner` does the rest.

1. A message is stored with a receipt. User messages newer than the last turn are the queue.
2. When the agent is free it starts one turn over the whole queue, checks the context size against the model window, sends the thread's messages with the tool specs and streams the answer. When the model calls tools, they run one by one and the next step sends their results back, until the model answers without calling a tool (at most 50 steps). Each step, the calls and their results, is stored in one transaction.
3. `send` during a turn waits for the next one; `send_now` stops the turn, keeping the text written so far, and the next turn starts at once; `cancel` only stops. Messages sent during a turn are stored after its answer, so the thread reads in the order things happened.
4. A turn ends `completed`, `interrupted` or `failed`. A turn left running by a stopped process is marked interrupted on the next start.
5. Every successful write publishes an `Event` to `State::subscribe` receivers.

## Files

```text
~/.config/duskcode/
    config.yml               providers and session settings (mode 0600)
    MAIN.md                  system prompt of the main agent (mode 0600)
    skills/**/SKILL.md       skills, named by their path: skills/git/notes -> git/notes
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
    # inline_tags: true   # the server leaves <think>/<tool_call> tags in the text

session:
  record: full      # user messages and the whole model output (default)
  # record: answer  # user messages and model answers only

tools:              # every tool is on unless switched off
  run_command: false
```

The core loads the config once into `LiveConfig`: readers take a snapshot, changes such as `:off run_command` are written to disk atomically and applied in memory, and a background check re-reads the file within five seconds of a hand edit (`:reload` does it at once). A broken file is reported and the last good config stays in use. A rewrite keeps only the core's own header comment. Each agent step takes a fresh snapshot, so switches apply from the next step; a switched-off tool is neither offered to the model nor run.

The CLI uses the first provider and its first catalogue model and starts a new session per run. `--state-db PATH` overrides the database location.

## Skills

Any folder under `skills/` with a `SKILL.md` is a skill, named by its path. The file opens with front matter; the rest is the instructions:

```markdown
---
description: Preparing a release           # what the model reads to decide
entry: true                                # offered before any skill is loaded
links: [git/release-notes, git/changelog]  # skills this one leads to
---
Steps the agent follows...
```

The core reads the tree on every step, so new and edited skills apply at once, and builds the graph itself: broken files and links to missing skills are listed by `:skills` and never shown to the model. The model sees the entry skills in the description of `load_skill`; loading a skill returns its instructions and the skills it links to, so the model walks the graph one step at a time instead of reading every skill up front. A switched-off skill is hidden everywhere. Skills are switched like tools, in the `skills:` section or with `:on` and `:off`.

## Guarantees

- MAIN.md is copied into a new thread as its first, `system` message; editing the file affects only chats started later. A missing or blank file means no system prompt.
- The model receives only role and text; IDs, timestamps, cwd and attachments stay in the core. Reasoning is stored but never sent back as context.
- Text is stored once, in `messages`; turns refer to messages and every request is rebuilt from them. Each message carries an estimated token count (UTF-8 bytes / 4); providers report usage per request only.
- At most one turn runs per thread, enforced by the database.
- Tools run without asking. File tools only reach the directory the client was started in and the session workspace. `run_command` runs in the client's working directory with a timeout and output caps in its own process group, so nothing it started outlives it; otherwise it can do what the user can.
- A request UID is idempotent: a retry on the same thread returns the original receipt without a new event. Rejected requests get a fixed code (`empty_message`, `empty_thread`, `unknown_thread`) and are not queued.
- Every provider turns its stream into one `ModelEvent` type: reasoning, text, tool call starts and argument pieces, usage and the finish reason. `ModelOutput` assembles them; a response cut by the token limit or a filter is an error.
- Provider responses are untrusted: sizes and idle time are bounded, redirects are refused, and upstream text never reaches error messages.
- Events are in-process only: no replay, and slow receivers can miss events.

## Limitations

Skills and agent prompts are not loaded yet. Skills are text only: files next to a `SKILL.md` are not reachable by the agent. No permissions, MCP servers, context compaction, session resume or retries yet. Messages sent during a turn live in memory until it ends. An interrupted turn keeps its text only; reasoning without text is not stored. Schemas are standalone per table without upgrade migrations, so a schema change needs a fresh database.
