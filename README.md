# dusk_code

An AI agent system in Rust. All logic lives in one core library, `dusk_core`; front ends (the CLI today, a TUI and GUI later) are thin clients over it.

```sh
cargo run -p dusk_code
cargo test --workspace
```

## Words

- **thread**: one chat. It holds messages and turns.
- **message**: one entry in a thread: the system prompt, a user message, a model answer or a tool result.
- **turn**: one run of the agent. It answers every user message that waited in the queue.
- **step**: one model call inside a turn. A step that calls tools is stored with the tool results; the next step sends them back to the model.

## Layout

```text
projects/
    core/src/                dusk_core
        lib.rs               module list
        api.rs               core version
        paths.rs             every application path
        init.rs              first-start setup of those paths
        config.rs            config.yml: providers, recording, tool and skill switches
        live_config.rs       the current config in memory, written back on change
        thread_start.rs      starts a thread: workspace folder, record, MAIN.md
        skills.rs            the skill graph read from skills/
        state/               SQLite records, one file per table
            state.rs         database handle: open, close
            threads.rs       threads
            messages.rs      messages, in order
            turns.rs         turns and their steps
            tokens.rs        token counts by author and provider usage per thread
            migrations/      one standalone schema file per table
        runtime/             what clients start, and the agent
            runtime.rs         Runtime: database, config, model, background tasks, threads
            thread_runner.rs   a live thread: send, send now, cancel, events
            inbox.rs           client commands while the agent works
            agent.rs           a turn, step by step: model -> tools -> model
            generation.rs      one model call, streamed into a caller-owned output
            context.rs         stored messages -> model input and its token size
        tools/               what the agent can call
            tool.rs          the Tool interface and what a tool sees
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
        main.rs, app.rs      starts the runtime, opens a thread, shuts down
        chat.rs              line-based chat: `text` queues, `!text` sends now, `!` stops
        commands.rs          `:tools`, `:skills`, `:on <name>`, `:off <name>`, `:reload`
```

## How the core runs

A client starts one `Runtime`. It sets up missing files, opens the database and the config, picks the model, and checks the config file once a minute in a background task. `Runtime::new_thread` starts a thread and gives it its own task; the client only sends text with `ThreadInput` and reads `ThreadEvent`s from `ThreadOutput`. A thread task ends when the client drops its input and the queue is done; `Runtime::shutdown` waits for that, stops the background task and closes the database.

## How a thread works

1. A user message is stored. User messages newer than the last turn are the queue.
2. When the agent is free it starts one turn over the whole queue, checks the context size against the model window, sends the thread's messages with the tool specs and streams the answer. When the model calls tools, they run one by one and the next step sends their results back, until the model answers without calling a tool (at most 50 steps). Each step, the calls and their results, is stored in one transaction.
3. A model call that fails before writing anything, on a network error, a timeout, a rate limit, a server error or a cut stream, is tried up to three times in all, after pauses of one and two seconds; the client sees each failed attempt as `Retrying`. Other failures, such as a rejected API key, end the turn at once, and the client sees the error.
4. `send` during a turn waits for the next one; `send_now` stops the turn, keeping the text written so far, and the next turn starts at once; `cancel` only stops. Messages sent during a turn are stored after it ends, so the thread reads in the order things happened.
5. A turn ends `completed`, `interrupted` or `failed`. A turn left running by a stopped process is marked interrupted on the next start.
6. After each turn the client gets `Tokens`, three different numbers:
   - `current`: the estimated size of what the model receives now, by author: system, user, agent, tool calls, tool results. It holds the messages after the thread's `context_from`; compacting will move that point.
   - `all`: the same over the whole thread history, reasoning included.
   - `spent`: the sums of what the provider reported per model call: prompt (and its cached part) and completion (and its reasoning part). Every call sends the whole context again, so this grows much faster than the thread.

## Files

```text
~/.config/duskcode/
    config.yml               providers, recording, tool and skill switches (mode 0600)
    MAIN.md                  system prompt of the main agent (mode 0600)
    skills/**/SKILL.md       skills, named by their path: skills/git/notes -> git/notes
    session/state.db         threads, turns and messages (mode 0600)
~/Documents/duskcode/
    <thread id>/             thread workspace: temporary and downloaded files (mode 0700)
```

On first start the core creates the directories (mode 0700), a config template without providers, a default MAIN.md and an empty database; existing files are never modified. Add a provider and restart:

```yaml
providers:
  main:
    kind: openai_compatible
    base_url: 'https://api.example.com/v1'
    api_key: 'sk-...'
    # inline_tags: true   # the server leaves <think>/<tool_call> tags in the text

thread:
  record: full      # user messages and the whole model output (default)
  # record: answer  # user messages and model answers only

tools:              # every tool is on unless switched off
  run_command: false
```

The core loads the config once into `LiveConfig`: readers take a snapshot, changes such as `:off run_command` are written to disk atomically and applied in memory, and a background check re-reads the file within a minute of a hand edit (`:reload` does it at once). A broken file is reported and the last good config stays in use. A rewrite keeps only the core's own header comment. Each agent step takes a fresh snapshot, so switches apply from the next step; a switched-off tool is neither offered to the model nor run.

The CLI uses the first provider and its first catalogue model and starts a new thread per run. `--state-db PATH` overrides the database location.

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
- Text is stored once, in `messages`; turns refer to messages and every request is rebuilt from them. Each message carries estimated token counts (UTF-8 bytes / 4) for its text, tool calls and reasoning; providers report usage per request only.
- At most one turn runs per thread, enforced by the database.
- Tools run without asking. File tools only reach the directory the client was started in and the thread workspace. `run_command` runs in the client's working directory with a timeout and output caps in its own process group, so nothing it started outlives it; otherwise it can do what the user can.
- A user message carries a request UID; sending it again returns the stored message instead of adding a second one.
- Every provider turns its stream into one `ModelEvent` type: reasoning, text, tool call starts and argument pieces, usage and the finish reason. `ModelOutput` assembles them; a response cut by the token limit or a filter is an error.
- Provider responses are untrusted: sizes and idle time are bounded, redirects are refused, and upstream text never reaches error messages.

## Limitations

Subagents are not implemented yet. Skills are text only: files next to a `SKILL.md` are not reachable by the agent. No permissions, MCP servers, context compaction or thread resume yet. Token estimates leave out tool definitions and chat template markup, which the provider counts. Messages sent during a turn live in memory until it ends. An interrupted turn keeps its text only; reasoning without text is not stored. Schemas are standalone per table without upgrade migrations, so a schema change needs a fresh database.
