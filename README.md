# duskcode

duskcode is a terminal coding agent written in Rust. It connects to model APIs and
provides tools for reading and editing project files, running shell commands, and
delegating tasks to specialist agents. Sessions are stored locally in SQLite.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
```

The installer downloads a release build for macOS arm64 or Linux x86_64 / arm64,
verifies its checksum, and links `duskcode` into `/usr/local/bin` or `~/.local/bin`.
Run it from a project folder:

```sh
duskcode
```

Use `/connect` to configure a provider and `/model` to select a model. The supported
protocols are OpenAI-compatible Chat Completions and Anthropic Messages.

To build from source:

```sh
cargo build --release
./target/release/duskcode
```

<p align="center">
  <img src="docs/screenshots/quiz.gif" alt="duskcode running a task and displaying a question" width="800">
</p>

## Features

- MAIN, ORCHESTRATOR, and PLAN modes for direct work, delegation, and planning.
- Specialist agent definitions, skills, modes, and prompts stored as editable Markdown.
- MCP server connections and configurable tools and agents through `/settings`.
- A transcript of responses, reasoning, and tool calls, with context and token usage.
- Background shell commands with status and output in `/jobs`.
- Task tracking, plus goals with acceptance criteria through `/goal`.
- Saved chats through `/resume`, context summaries through `/compact`, and new threads
  from a summary through `/fork`.
- Configuration and chat export through `/migrate`; archives include API keys.
- Release updates through `/update`, with stable and dev channels.

![duskcode terminal interface](docs/screenshots/01-overview.png)

## Architecture

```text
duskcode TUI
     │
 dusk_core
     ├── Model providers: OpenAI-compatible / Anthropic
     ├── Agent runtime and tools: files / shell / MCP
     └── Local state and configuration: SQLite / YAML / Markdown
```

| Component | Responsibility |
| --- | --- |
| `projects/core` | Configuration, providers, agents, tools, runtime, and state |
| `projects/tui` | Terminal interface, input, navigation, and images |
| `projects/evals` | Evaluation runner for coding tasks |
| `resources/` | Bundled modes, agents, skills, and prompts |

## Configuration and data

| Location | Contents |
| --- | --- |
| `~/.config/duskcode/config.yml` | Providers, model, limits, tools, agents, skills, MCP servers, appearance, and logging |
| `~/.config/duskcode/rules/` | Agents, skills, modes, prompts, and user memory |
| `~/.config/duskcode/session/` | Session database and artifacts |
| `~/.config/duskcode/logs/` | Application logs |
| `~/.config/duskcode/mcp-auth/` | MCP OAuth tokens |
| `~/Documents/duskcode/` | Chat workspaces and files; `projects/` holds project memory |
| `~/.local/share/duskcode/` | Default installation directory; the installer also supports `XDG_DATA_HOME` |

`DUSKCODE_LOG` sets the log level. Logging can be disabled with `logging.enabled: false`;
optional OpenTelemetry tracing is configured under `logging.traces`.
`DUSKCODE_GRAPHICS=kitty|iterm2|sixel` selects a terminal image protocol.
`duskcode remove` starts the interactive uninstaller.

## Limitations

- The project is pre-1.0; commands, configuration, and database schema may change.
- Shell commands run with the user's permissions. The approval setting controls
  confirmation prompts; it does not provide an operating-system sandbox.
- Model behavior and API compatibility depend on the configured provider and model.
  Requests sent to external providers and MCP servers are subject to their data policies.
- Release builds target Linux and macOS; there is no Windows build.
- Image display depends on terminal graphics support, with a character preview fallback.

## License

[MIT](LICENSE). Bundled skills retain their own licenses; see
[resources/skills/NOTICE.md](resources/skills/NOTICE.md).
