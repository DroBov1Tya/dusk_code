# dusk_code

A Rust AI agent application with a shared core library, `dusk_core`, and a terminal client, `duskcode`.

## Features

- OpenAI-compatible providers with streaming model responses.
- Conversation threads and messages stored in SQLite; existing threads can be resumed.
- Queued messages, interruption, and turn cancellation.
- Manual and automatic context compaction, with access to earlier thread history.
- `/fork`: a detailed summary of the thread (`FORK.md` says how) starts a new one.
- Paths typed in a message (`/`, `~/`, `./`, `../` after a space) list their folder above
  the box; Tab walks the entries. Large folders are read in part (50 000 names, 200
  matches), so typing more narrows them.
- `/undo`: pick one of your messages with the arrows; the thread goes back to before
  it (later messages, turns and compactions are removed) and its text returns to the
  box to edit and send again.
- `/goal <text>`: a standing goal. Every request carries it, and a turn that ends
  without it done starts the next round, up to `limits.max_goal_rounds` (10, set in
  `/settings` -> agents). The model marks it done, pauses or sets one with the `goal`
  tool; `/goal pause|resume|clear|done` and the buttons above the box do the same. A
  stop or a failed turn pauses it.
- File reading, writing, editing and deletion; shell commands; loading skills from `SKILL.md` files.
- `/effort off|low|medium|high|default`; off is sent as `none`, and a provider that
  rejects it gets the previous effort back. `/settings` -> models sets the input budget
  per model: Auto, 128k to 1M, or a token count, capped by the model window.
- `display_image`: the chat shows a public HTTPS PNG or JPEG as an image card, kept
  with a thumbnail beside the records; plain URLs in messages are never fetched.
  Cards use half-block previews by default; `DUSKCODE_GRAPHICS=kitty|iterm2|sixel`
  opts into terminal pictures, and `off` keeps the previews. Inside tmux pictures
  need `allow-passthrough`.
- Markdown tables in answers render with aligned columns, stacked when narrow.
- Background subagents with separate threads and messages to their parent.
- Background commands: `run_command` with `background=true` returns a job id at once,
  and the result joins the chat by itself when the command ends, even after the turn
  did. The `jobs` tool lists, shows and cancels them. At most 8 run per thread; each
  stream keeps 32 KiB. A job is never restarted: after a crash a running one is marked
  interrupted. The chat shows the first lines of a result, the agent gets all of it.
- `/update` shows the download and install as it goes; iTerm2 shows what the agent
  is doing in its tab (session status and native progress).
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
- `/settings` -> system: memory and processor use of duskcode, and what the database,
  image cards and logs take on disk, read every 2 seconds while open.
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
