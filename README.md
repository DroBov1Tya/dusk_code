# dusk_code

A Rust AI agent application with a shared core library, `dusk_core`, and a terminal client, `dusk`.

## Features

- OpenAI-compatible providers with streaming model responses.
- Conversation threads and messages stored in SQLite; existing threads can be resumed.
- Queued messages, interruption, and turn cancellation.
- Manual and automatic context compaction, with access to earlier thread history.
- File reading, writing, editing and deletion; shell commands; loading skills from `SKILL.md` files.
- Background subagents with separate threads and messages to their parent.
- Persistent task boards displayed in the terminal sidebar.

## License

MIT, see [LICENSE](LICENSE).

The built-in skills are written by Anthropic, OpenAI and Nous Research (Hermes Agent) and keep
their own licenses; see [projects/core/skills/NOTICE.md](projects/core/skills/NOTICE.md).
