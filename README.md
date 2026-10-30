# dusk_code

A Rust AI agent application with a shared core library, `dusk_core`, and a terminal client, `duskcode`.

## Features

- OpenAI-compatible providers with streaming model responses.
- Conversation threads and messages stored in SQLite; existing threads can be resumed.
- Queued messages, interruption, and turn cancellation.
- Manual and automatic context compaction, with access to earlier thread history.
- File reading, writing, editing and deletion; shell commands; loading skills from `SKILL.md` files.
- Background subagents with separate threads and messages to their parent.
- Persistent task boards displayed in the terminal sidebar.

## Install

macOS on Apple silicon, Linux on x86_64 and arm64:

```sh
curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
```

The script checks the release checksum, puts the binary in `~/.local/share/duskcode/bin`
and links `duskcode` from `/usr/local/bin`, or `~/.local/bin` when that is not writable.
Run `duskcode` in the folder to work in. `duskcode remove` asks once, then deletes the
binary, its link and `~/.config/duskcode`.

## Logs

The core writes `~/.config/duskcode/logs/core.log` and the terminal client
`~/.config/duskcode/logs/tui.log`, as plain text. `DUSKCODE_LOG` sets the level the way
`RUST_LOG` does (`DUSKCODE_LOG=debug duskcode`); `info` by default. Tool arguments,
outputs and secrets are not logged.

## License

MIT, see [LICENSE](LICENSE).

The built-in skills are written by Anthropic, OpenAI and Nous Research (Hermes Agent) and keep
their own licenses; see [projects/core/skills/NOTICE.md](projects/core/skills/NOTICE.md).
