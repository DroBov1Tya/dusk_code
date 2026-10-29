# dusk_code

An AI agent system in Rust. All logic lives in one core library, `dusk_core`; front ends are thin clients over it. The first client is `dusk`, a full-screen terminal app.

## Quick start

Needs Rust 1.85 or newer and an OpenAI-compatible endpoint.

```sh
git clone https://github.com/DroBov1Tya/dusk_code
cd dusk_code
cargo run --release
```

The first start creates `~/.config/duskcode/` and stops. Add a provider to `config.yml` and run again:

```yaml
providers:
  main:
    kind: openai_compatible
    base_url: 'https://api.example.com/v1'
    api_key: 'sk-...'
```

`cargo install --path projects/tui` puts `dusk` on the `PATH`.

## Features

- Threads survive crashes: messages and tool calls are stored before they are acted on.
- Messages can be queued while the agent works, or sent at once, interrupting it.
- A paste over 1500 characters shows as one chip in the box and as a folded block in the thread; the model gets it whole, in `<pasted>` tags.
- `/compact` and automatic compaction at 80% of the input limit; old messages stay readable with `read_history`.
- Input and output token limits per model call, enforced locally.
- Subagents: `ask_agent` hands a task to an agent working in the background in a thread of its own; calling it again with that thread writes to the agent while it works. The agent writes back with `message_parent`, and its final answer arrives on its own. Messages go through the database and are read between steps; Esc stops the agents too. The built-in `general` agent needs no setup; others are `agents/**/AGENT.md` files.
- Skills: `skills/**/SKILL.md` files the agent loads with `load_skill`.
- Built-in tools: read, write, edit and delete files, read the thread history, run commands.

## Configuration

```text
~/.config/duskcode/
    config.yml        providers, limits, tool and skill switches
    MAIN.md           system prompt of the main agent and of general
    COMPACT.md        how a thread is summarized
    skills/           SKILL.md files
    agents/           AGENT.md files
    session/state.db  threads, messages, usage
```

| Key | Default | Meaning |
|---|---|---|
| `limits.max_input_tokens` | `100000` | the largest request |
| `limits.max_output_tokens` | `100000` | the most the model writes in one call |
| `thread.record` | `full` | `answer` keeps only answers, not reasoning |
| `tools.<name>`, `skills.<name>` | on | `false` switches one off |

An agent file:

```markdown
---
description: Reviews a diff for bugs
entry: true                       # the chat may ask it
links: [tester]                   # agents it may ask
tools: [read_file, run_command]   # all when left out
---
You review code...
```

## Keys

| Key | Does |
|---|---|
| Enter / Alt+Enter | send; interrupt and send now |
| Esc | stop the turn |
| Ctrl+C | clear the box; twice within 5 s quits |
| Tab, click | open or close thinking, tool calls, summaries |
| drag | select text and copy it |
| hover | `copy` a message, `edit` one you sent |
| Ctrl+E, click | edit a pasted text in its own window; Ctrl+S saves, Esc asks about unsaved changes |
| Ctrl+T or `/resume` | threads, with a preview of the selected one |
| Ctrl+N | new thread |
| Ctrl+O | switch tools and skills |
| F2 / F1 | sidebar / every key |

`--thread ID` opens a thread at start, `--state-db PATH` uses another database.

## Limitations

Tools run without approval; file tools are confined to the working directory and the thread workspace, `run_command` is not. No MCP servers yet. Token counts before the first model call of a thread are estimates.

## License

MIT, see [LICENSE](LICENSE).
