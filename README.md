# dusk_code

An AI agent system in Rust. All logic lives in one core library, `dusk_core`; front ends are thin clients over it, as Linux distributions are over one kernel. The first client is `dusk`, a full-screen terminal app.

```text
╭ statistics ──────────────╮│ ◒ dusk  ·  Poet Agent Tea Poem Request
│ model qwen3.8-flash      ││
│ context 2%               ││ ▌ Ask the poet agent for a two-line poem about tea.
│ 1.8k / 100k              ││
│ in 3.9k   out 245        ││   ▸ thinking · ~10 tokens
│ cache 1.7k  think 125    ││   ◆ ask_agent  poet: Write a two-line poem about tea.   ✓ 3 lines
│ calls 3  last 00:07      ││   ▸ thinking · ~43 tokens
╰──────────────────────────╯│
╭ subagents ───────────────╮│   The poet came back with:
│▀▀█▀▀ poet                ││
│ ▄▀▄  Cinder-fc 00:02 done││   │ Steam lifts its slow calligraphy from the cup,
│▄▄▄▄▄ Write a two-line …  ││   │ and the leaf remembers every rain it drank.
```

## Quick start

Needs Rust 1.85 or newer (edition 2024) and an OpenAI-compatible endpoint.

```sh
git clone https://github.com/DroBov1Tya/dusk_code
cd dusk_code
cargo run --release
```

The first start creates `~/.config/duskcode/` with a config template and stops: add a provider to `config.yml` and run again.

```yaml
providers:
  main:
    kind: openai_compatible
    base_url: 'https://api.example.com/v1'
    api_key: 'sk-...'
```

`cargo install --path projects/tui` puts `dusk` on the `PATH`. `cargo test --workspace` runs the tests.

## What it does

- **Threads that survive crashes.** Messages are stored before they are acted on, tool calls before they run, results right after. A restart picks up where the process stopped.
- **Queue, interrupt, stop.** Messages can be sent while the agent works: queued for the next turn, or sent now, stopping the running one and keeping what it wrote.
- **Context that fits.** Token counts calibrated by the provider's own count; `/compact` and automatic compaction at 80% of the input limit; compacted messages stay readable with `read_history`.
- **Limits on both sides.** Input and output caps per model call, enforced locally even when the provider ignores them.
- **Subagents.** `ask_agent` hands a task to an agent working in a thread of its own, with its own context and tools. A built-in `general` agent works without any setup; more are `AGENT.md` files.
- **Skills.** A graph of `SKILL.md` files the agent walks one step at a time with `load_skill`.
- **Few built-in tools.** Read, write, edit and delete files, read the thread's history, run commands. Everything else belongs to MCP servers.
- **Live config.** Tools and skills switch on and off while running; hand edits are picked up within a minute.

## Architecture

```text
   dusk (TUI)                  other clients, later
       │                              │
       └──── ThreadInput ─▶ ◀─ ThreadEvent ┘
                          │
┌───────────────────────── dusk_core ──────────────────────────┐
│ Runtime ─▶ ThreadRunner ─▶ Agent                             │
│                              │  step: model ─▶ tools ──┐     │
│                              │  ◀─────── results ──────┘     │
│                              └─ ask_agent ─▶ ThreadRunner    │
│                                              of the subagent │
│                                                              │
│ state/  SQLite with FTS5      providers/  OpenAI-compatible  │
└──────────────────────────────────────────────────────────────┘
```

| Part | What it does |
|---|---|
| `runtime/` | `Runtime` starts threads in tasks of their own; `ThreadRunner` takes client input, `Agent` runs turns step by step |
| `state/` | SQLite records, one file per table, written ahead of every action |
| `providers/` | the OpenAI-compatible client, its stream parser and one `ModelEvent` type for every provider |
| `tools/` | the tool interface, the registry and the built-in tools, confined to the working directory and the thread workspace |
| `graph.rs` | skills and agents, read from `skills/` and `agents/` |
| `tui/` | the `dusk` binary: app state with no drawing in `app/`, drawing in `ui/` |

### Words

- **thread**: one chat. It holds messages and turns.
- **message**: one entry in a thread: the system prompt, a user message, a model answer or a tool result.
- **turn**: one run of the agent. It answers every user message that waited in the queue.
- **step**: one model call inside a turn. A step that calls tools is stored with the tool results; the next step sends them back to the model.

<details>
<summary>Every file</summary>

```text
projects/
    core/src/                dusk_core
        lib.rs               module list
        api.rs               core version
        paths.rs             every application path
        init.rs              first-start setup of those paths
        commands/            `/compact` and other commands a message can be
        config.rs            config.yml: providers, recording, tool and skill switches
        live_config.rs       the current config in memory, written back on change
        thread_start.rs      starts a thread: workspace folder, record, MAIN.md
        graph.rs             skills and agents: the graph read from skills/ and agents/
        state/               SQLite records, one file per table
            state.rs         database handle: open, close
            threads.rs       threads, their titles and the list by activity
            messages.rs      messages, in order
            turns.rs         turns and their steps, written ahead
            pending.rs       what clients sent, before the thread takes it in
            compactions.rs   summaries that replace older messages in the context
            tokens.rs        token counts by author and provider usage per thread
            migrations/      one schema file per table, then one per change
        runtime/             what clients start, and the agent
            runtime.rs         Runtime: database, config, model, background tasks, threads
            launcher.rs        what a thread is built from: tools, scope, settings
            subagent.rs        ask_agent: a task for another agent in a thread of its own
            title.rs           naming a thread from its first message
            thread_runner.rs   a live thread: send, send now, cancel, events
            inbox.rs           client input while the agent works
            agent.rs           a turn, step by step: model -> tools -> model
            compaction.rs      summarizing the context, on /compact or near the window
            generation.rs      one model call, streamed into a caller-owned output
            context.rs         stored messages and summary -> model input and its token size
        tools/               what the agent can call
            tool.rs          the Tool interface and what a tool sees
            registry.rs      tools by name; failed calls become error results
            scope.rs         where tools may act, checked after following symlinks
            files.rs         read_file, write_file, edit_file, delete_file
            shell.rs         run_command: sh -c in the working directory
            load_skill.rs    load_skill: walks the skill graph
            history.rs       read_history: the thread's own messages, by number or text
        providers/           models
            provider.rs      providers by name, model discovery, stream dispatch
            chat.rs          model input: messages by role, tool specs
            model_output.rs  model output: ModelEvent stream, ModelOutput assembly
            openai.rs        OpenAI-compatible client and stream parser
            inline_tags.rs   <think> and <tool_call> tags in text -> ModelEvents
            sse.rs           bounded Server-Sent Events decoder
    tui/src/                 dusk binary: the full-screen client
        main.rs, args.rs     starts the runtime and takes the screen
        session.rs           the loop: keys, thread events, opening and closing threads
        clipboard.rs         copying: a system tool or the OSC 52 sequence
        app/                 what the client knows, with no drawing
            mod.rs           the open thread, activity, tokens; applies thread events
            transcript.rs    entries: messages, foldable thinking, tool calls, summaries
            input.rs         the message being typed
            keys.rs          keys and mouse -> changes or actions for the session
            panels.rs        threads, tool and skill switches, keys
        ui/                  drawing, one file per part of the screen
            theme.rs         the dusk palette and glyphs
            header.rs        the thread title
            sidebar.rs       the left rail: statistics, subagents, MCP tools
            transcript.rs    the conversation, folded and wrapped
            markdown.rs      headings, lists, quotes, code in answers
            input.rs         the status rule, the message box, the command list
            panels.rs        panels over the transcript
            wrap.rs          fitting styled text into a width
            screen.rs        taking the terminal and giving it back
```

</details>

## The full-screen client

`dusk` shows one thread at a time, with its title on top. The left rail starts with statistics: the model, the share of the context in use, the tokens used and the limit, what the provider counted in and out, cached and thinking tokens, model calls, how long the turn runs and how many tokens a second arrive. Sections for subagents and MCP tools appear below it only when there is something to show. F2 hides the rail; on a narrow screen it opens over the transcript instead.

The client takes the mouse, so it selects text itself: dragging over the transcript marks text, and releasing copies it, without the bars and indents on the left. A message under the mouse shows its buttons: `copy` takes its whole text, folded or not, and `edit` puts a message you sent back into the box. Copying goes through `pbcopy`, `wl-copy` or `xclip` where one exists, and through the terminal's OSC 52 sequence, which also works over SSH.

Thinking, tool calls and compaction summaries fold into one line each. Tab walks them, Enter or a click opens or closes one, Ctrl+R opens or closes all thinking. Messages can be sent while the agent works: Enter queues for the next turn, Alt+Enter interrupts, Esc stops. Ctrl+T lists the threads and opens one with its history, Ctrl+N starts a new one, Ctrl+O switches tools and skills, and F1 lists every key. A new thread is stored only when its first message is sent. `--thread ID` opens a thread at start.

| Key | Does |
|---|---|
| Enter | send; during a turn, queue for the next one |
| Alt+Enter | interrupt the turn and send now |
| Esc | stop the turn |
| Tab, Enter or click | walk folded entries, open or close one |
| drag | select text and copy it; Esc clears the selection |
| hover | `copy` on every message, `edit` on yours puts it back in the box |
| Ctrl+R | open or close all thinking |
| Ctrl+T | threads |
| Ctrl+N | new thread |
| Ctrl+O | switch tools and skills |
| F2 | hide or show the left rail |
| F1 | every key |

## Configuration

```text
~/.config/duskcode/
    config.yml               providers, recording, tool and skill switches (mode 0600)
    MAIN.md                  system prompt of the main agent (mode 0600)
    COMPACT.md               how to summarize a thread when its context is compacted
    skills/**/SKILL.md       skills, named by their path: skills/git/notes -> git/notes
    session/state.db         threads, messages, turns, compactions, usage (mode 0600)
~/Documents/duskcode/
    <thread id>/             thread workspace: temporary and downloaded files (mode 0700)
```

| Section | Key | Default | Meaning |
|---|---|---|---|
| `providers` | `kind`, `base_url`, `api_key` | none | OpenAI-compatible endpoints; `dusk` uses the first and its first model |
| `providers` | `inline_tags` | `false` | the server leaves `<think>` and `<tool_call>` tags in the text |
| `thread` | `record` | `full` | `full` keeps the whole model output, `answer` only answers |
| `limits` | `max_input_tokens` | `100000` | the largest request |
| `limits` | `max_output_tokens` | `100000` | the most the model writes in one call |
| `tools`, `skills` | `<name>: false` | on | switch one off |

On first start the core creates the directories (mode 0700), a config template without providers, a default MAIN.md and COMPACT.md and an empty database; existing files are never modified. A full config:

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

limits:             # tokens per model call
  max_input_tokens: 100000
  max_output_tokens: 100000

tools:              # every tool is on unless switched off
  run_command: false
```

The core loads the config once into `LiveConfig`: readers take a snapshot, changes such as switching `run_command` off are written to disk atomically and applied in memory, and a background check re-reads the file within a minute of a hand edit. A broken file is reported and the last good config stays in use. A rewrite keeps only the core's own header comment. Each agent step takes a fresh snapshot, so switches apply from the next step; a switched-off tool is neither offered to the model nor run.

`dusk` uses the first provider and its first catalogue model. `--thread ID` opens a thread instead of a new one, and `--state-db PATH` overrides the database location.

## How the core runs

A client starts one `Runtime`. It sets up missing files, opens the database and the config, picks the model, and checks the config file once a minute in a background task. `Runtime::new_thread` starts a thread and gives it its own task; the client only sends text with `ThreadInput` and reads `ThreadEvent`s from `ThreadOutput`.

For working with threads a client has three more calls: `list_threads` gives every thread with its title, the most recently active first; `thread_messages` gives the whole history of one, compacted messages included, to show it again; `resume_thread` picks it up with a new task. Resuming closes what a stopped process left open (a running turn becomes interrupted, a running tool call gets a result saying it may have run) and takes in messages that were sent but not yet taken in. A thread has at most one task, so opening it twice is refused.

A thread is named from its first message: at once with its first 35 characters and an ellipsis, then, after the first turn, by the model with a title of a few words. The client sees both as `TitleChanged`; when naming fails the stand-in stays. A thread task ends when the client drops its input and the queue is done; `Runtime::shutdown` waits for that, stops the background task and closes the database.

## How a thread works

1. A user message is stored. User messages newer than the last turn are the queue.
2. When the agent is free it starts one turn over the whole queue, checks the context size against the input limit, sends the thread's messages with the tool specs and streams the answer. When the model calls tools, they run one by one and the next step sends their results back, until the model answers without calling a tool (at most 50 steps). Each step is written ahead: its calls are stored before any tool runs, and each result right after its tool. A call left without a result, by a crash or a failed write, gets a result saying so when the turn ends or on the next start, since the model needs one per call and the tool may have had an effect.
3. A model call that fails before writing anything, on a network error, a timeout, a rate limit, a server error or a cut stream, is tried up to three times in all, after pauses of one and two seconds; the client sees each failed attempt as `Retrying`. Other failures, such as a rejected API key, end the turn at once, and the client sees the error.
4. `send` during a turn waits for the next one; `send_now` stops the turn, keeping the text written so far, and the next turn starts at once; `cancel` only stops. A send returns once the message is stored as pending, so a confirmed message survives a crash. The thread takes pending messages in when it is free, after the running turn or on the next start, so the thread reads in the order things happened.
5. A turn ends `completed`, `interrupted` or `failed`. A turn left running by a stopped process is marked interrupted on the next start.
6. After each turn the client gets `Tokens`:
   - `context`: the size of the next request, which the context check also uses. The provider counted the last request exactly, so only messages stored after it are estimated; before the first call everything is.
   - `current`: the estimated size of what the model receives now, by author: system, user, agent, tool calls, tool results, the compaction summary, and the tool definitions every request carries.
   - `all`: the same over the whole thread history, reasoning included.
   - `spent`: the sums of what the provider reported per model call: prompt (and its cached part) and completion (and its reasoning part). Every call sends the whole context again, so this grows much faster than the thread.

## Commands and compaction

A message that starts with a known `/word` is a command for the core, not text for the model; the rest of the line is its argument. `/etc/hosts` and other text that only looks like one stays a message, and an unknown command such as `/compat` is rejected instead of being sent. Commands sent during a turn run after it, in the order they were sent.

`/compact [focus]` summarizes the conversation. The model gets the context with the instructions from COMPACT.md (six sections: requested, completed and open tasks, the current task, paths and files, a summary) and the focus, if given. The summary then stands in for every message it covers: the model receives the system messages, the summary and the newer messages. Queued messages are never summarized away; they wait for their turn after the summary.

The core also compacts on its own once the context passes 80% of the input limit, so a 256k limit compacts at about 205k and the rest is room for the summary request itself. It checks before each turn and after each tool step, since tool results are what fills a context fastest; a turn goes on from the summary. A failed compaction leaves the context as it was.

Nothing is deleted: compacted messages stay in the database, count in `all` and can be read again by the agent with `read_history`, by message number or by any text of three characters or more (a trigram full-text index over message text and tool calls).

## Limits

Every model call has two limits, set in the `limits:` section of the config and 100k tokens each by default:

- `max_input_tokens`: the largest request. The input limit is this or the model window, whichever is smaller; the context is compacted at 80% of it and never sent past it. A message that alone is larger is refused when it is sent.
- `max_output_tokens`: the most the model may write in one call, reasoning included. It goes to the provider as `max_tokens`, and the core also counts what streams in and closes the connection past the limit, in case the provider ignores it. Either way the turn ends as interrupted with the text written so far, and the client gets `OutputLimitReached`. A stream cut this way reports no usage, so its cost is not counted.

## Subagents

Any folder under `agents/` with an `AGENT.md` is an agent, named by its path and read like a skill. Its text is the system prompt of the threads it works in; `tools` limits what it may use:

```markdown
---
description: Reviews a diff for bugs     # what the asking model reads to choose
entry: true                              # a chat may ask it
links: [tester]                          # agents it may ask in turn
tools: [read_file, run_command]          # left out: every tool
---
You review code...
```

Without any `AGENT.md` there is still `general`, a built-in agent that works like a chat: MAIN.md and every tool. It is what `ask_agent` asks when no agent is named, so the model can delegate from the first start; an `agents/general` folder replaces it.

The model hands work over with `ask_agent`: the agent's name, or none for `general`, and the whole task, since the agent sees nothing of the asking conversation. The agent works in a thread of its own under the asking one, with its own context, compaction and token count, and its final answer comes back as the tool result, starting with the agent's thread. Passing that thread again goes on with the same conversation; only threads the asker opened with that agent are accepted. A chat may ask the entry agents, an agent the agents it links to, and three levels down no agent may ask another. Stopping the asking turn stops the agent with it. Subagent threads are not listed among the chats; what they cost counts in the asking thread's `spent`.

While an agent works, the asking thread reports it to its client: `SubagentStarted` names the call, the agent's thread and the task, and every event of that thread follows wrapped in `Subagent`, an agent's own agents wrapped once more. `dusk` gives each a card in the left rail with a random name and a pixel figure drawn from its thread, the time it runs and the task.

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

The core reads the tree on every step, so new and edited skills apply at once, and builds the graph itself: broken files and links to missing skills are left out and never shown to the model. The model sees the entry skills in the description of `load_skill`; loading a skill returns its instructions and the skills it links to, so the model walks the graph one step at a time instead of reading every skill up front. A switched-off skill is hidden everywhere. Skills are switched like tools, in the `skills:` section.

## Guarantees

- MAIN.md is copied into a new thread as its first, `system` message; editing the file affects only chats started later. A missing or blank file means no system prompt.
- The model receives only role and text; IDs, timestamps, cwd and attachments stay in the core. Reasoning is stored but never sent back as context.
- Text is stored once, in `messages`; turns refer to messages and every request is rebuilt from them. Each message carries estimated token counts (UTF-8 bytes / 4) for its text, tool calls and reasoning; providers report usage per request only.
- At most one turn runs per thread, enforced by the database.
- Built-in tools are only the essentials: `read_file`, `write_file`, `edit_file` and `delete_file`, `read_history` over the thread itself, `run_command` for everything a terminal does (listing, searching, building), and `load_skill` for the skill graph. Anything more, such as web search, belongs to MCP servers.
- Tools run without asking. File tools only reach the directory the client was started in and the thread workspace. `run_command` runs in the client's working directory with a timeout and output caps in its own process group, so nothing it started outlives it; otherwise it can do what the user can.
- Every sent message carries a request UID from the moment it is stored as pending; taking it into the thread twice, after a crash between the two writes, adds it once.
- Every provider turns its stream into one `ModelEvent` type: reasoning, text, tool call starts and argument pieces, usage and the finish reason. `ModelOutput` assembles them; a response cut by the output limit ends the turn as interrupted, one blocked by a filter is an error.
- Provider responses are untrusted: sizes and idle time are bounded, redirects are refused, and upstream text never reaches error messages.

## Limitations

Skills are text only: files next to a `SKILL.md` are not reachable by the agent. No permissions or MCP servers yet. Estimates leave out the chat template markup, so before the first model call of a thread the context size is too low, and per-author counts stay estimates. An interrupted turn keeps its text only; reasoning without text is not stored. Schema changes are new migrations from 0008 on, so an existing database is upgraded on start.

## License

MIT, see [LICENSE](LICENSE).
