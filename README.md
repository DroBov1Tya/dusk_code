# dusk_code

A Rust AI agent application with a shared core library, `dusk_core`, and a terminal client, `duskcode`.

## Features

- OpenAI-compatible providers with streaming model responses.
- Conversation threads and messages stored in SQLite; existing threads can be resumed.
- Queued messages, interruption, and turn cancellation.
- Manual and automatic context compaction, with access to earlier thread history.
- `/fork`: a detailed summary of the thread (`FORK.md` says how) starts a new one.
- `/goal <text>`: a standing goal. Every request carries it, and a turn that ends
  without it done starts the next round, up to `limits.max_goal_rounds` (10, set in
  `/settings` -> agents). The model marks it done, pauses or sets one with the `goal`
  tool; `/goal pause|resume|clear|done` and the buttons above the box do the same. A
  stop or a failed turn pauses it.
- File reading, writing, editing and deletion; shell commands; loading skills from `SKILL.md` files.
- Background subagents with separate threads and messages to their parent.
- Modes switched with Tab: MAIN works itself, PLAN only reads and plans, ORCHESTRATOR
  hands work to agents. Each is a prompt in `~/.config/duskcode/modes/` with its own
  tool switches; any other `.md` there is a mode too. Tools follow the mode at once; the
  prompt is set at the thread's start and after each compaction, so the cached context
  stays the same in between.
- Persistent task boards displayed in the terminal sidebar.
- Expert agents in `agents/<name>/AGENT.md` and skills in `skills/<name>/SKILL.md`, both
  editable in `/settings`; agents may improve skills and write new ones from repeated,
  verified practice.
- MCP servers over stdio or HTTP, added and switched in `/settings`.
- Raw log files for the core and the terminal client.
- Questions with options the agent asks above the message box (`ask_user`); the answer
  goes back as `QUESTION: ... ANSWER: ...`, and a message typed instead declines it.
- Memory of the user's lasting preferences in `~/.config/duskcode/MEMORY.md`, kept
  short by the `remember` tool; switched in `/settings` -> personalize, sized by
  `memory.max_chars` in config.yml.

## Install

macOS on Apple silicon, Linux on x86_64 and arm64:

```sh
curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
```

The script checks the release checksum, puts the binary in `$XDG_DATA_HOME/duskcode/bin`
(default `~/.local/share/duskcode/bin`), and links `duskcode` from `/usr/local/bin`, or `~/.local/bin` when that is not writable.
Releases are the round versions (0.50.0, 0.60.0); every tag between them is a
pre-release, installed by name with `... | DUSKCODE_VERSION=v0.53.0 sh`.
`/update` or `duskcode --update` installs the newest release in place (or the newest
pre-release on the dev channel, set in `/settings` -> updates).

Shipped files follow the binary at the next start; your edits are kept, with the new
version beside them as `<file>.new`. Upgrading from a binary-only updater requires
rerunning the installer once to install the matching resources.

Run `duskcode` in the folder to work in. `duskcode remove` asks once, then deletes the
binary, its link, installed resources and `~/.config/duskcode`.

## Logs

The core writes `~/.config/duskcode/logs/core.log` and the terminal client
`~/.config/duskcode/logs/tui.log`, as plain text. `DUSKCODE_LOG` sets the level the way
`RUST_LOG` does (`DUSKCODE_LOG=debug duskcode`); `info` by default. Tool arguments,
outputs and secrets are not logged.

## License

MIT, see [LICENSE](LICENSE).

The built-in skills are written by Anthropic, OpenAI and Nous Research (Hermes Agent) and keep
their own licenses; see [resources/skills/NOTICE.md](resources/skills/NOTICE.md).
