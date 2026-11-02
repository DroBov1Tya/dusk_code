# duskcode

duskcode is a Rust coding agent for the terminal. A reusable core, configurable specialist agents, background commands and terminal image display keep coding work in one session.

MIT licensed. Built for Linux and macOS.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
```

For current resource-aware releases, the installer downloads a native executable and its versioned resources, verifies the release asset checksum, and installs a command link. Run `duskcode` in your project directory.

Use `/update` in the TUI or `duskcode --update` to install an update; restart to use the new version. The stable channel follows full releases, while the dev channel includes pre-releases. Select the channel in `/settings`, or install a particular tag with `DUSKCODE_VERSION=<tag>` passed to the installer.

Release builds are configured for macOS arm64 and Linux x86_64 / arm64 with musl. Windows builds are not provided.

## Parallel work you can follow

Specialist agents have their own instructions, context and tool access. Their editable delegation graph defines which roles may ask others for help. Agents can work in parallel and delegate further; open F3 to inspect active subagents without leaving the parent session.

Long shell commands can run in the background while the conversation continues. `/jobs` shows their status and output, and lets you stop a job. Completion returns to the chat. Jobs are not restarted automatically after a crash: an interrupted command requires an explicit decision to run it again.

The agent can maintain a persistent task board with individual steps and timings. Use it to separate implementation, verification and review, and see what remains open before accepting a result.

These features work together: delegate a repository audit, keep a slow command running, inspect each agent's work, and follow the remaining steps from the same terminal.

## A small terminal interface

The TUI keeps the conversation central. Thinking and tool calls collapse into compact rows; agent details, jobs and settings open when needed. Keyboard navigation and mouse interaction are available, with F1 showing the key map.

- **Context and effort:** `/context` shows the current context; `/effort` changes the requested reasoning effort for models that support it.
- **Session continuity:** `/resume` reopens a session; `/goal` sets a standing goal for continued work.
- **Sources:** `/sources` lists material attached to the thread, including long pastes saved in its workspace.
- **Images:** attach images as input, or ask the agent to display a public HTTPS PNG/JPEG. Rendering uses the terminal's available graphics protocol, with a fallback preview. This is image display, not image generation.
- **Local statistics:** `/settings` shows the process's memory, CPU and disk usage so the footprint can be inspected on your machine.

Native image support is detected automatically. `DUSKCODE_GRAPHICS=kitty|iterm2|sixel` overrides the choice; terminal and multiplexer support still matters.

## Rust, with bounded working sets

Dusk runs as a native Rust application without an interpreter runtime. Current resource-aware releases include the executable and a separate resources library; historical releases before 0.65.1 are binary-only.

Image decoding, image caches and retained background-command output have explicit limits. Streaming model responses arrive incrementally. These are implementation choices that constrain resource use; they are not a promise of a fixed RAM or disk footprint. Active sessions, output, images, platform and build configuration all affect measurements.

For a reproducible footprint comparison, record the release version, platform, terminal, provider, session state and measurement method. The system statistics in `/settings` provide a local starting point; no cross-platform RAM, startup-time or binary-size benchmark is claimed here.

## Core first

Shared application behavior lives in `dusk_core`. The current terminal client presents that behavior through a TUI; future clients can reuse the same core.

```text
             duskcode TUI
                  │
             dusk_core
                  │
    ┌─────────────┼──────────────┐
    │             │              │
 providers    agent runtime   local state
    │         and tools       and config
 model APIs   shell / MCP     sessions / YAML
```

| Component | Responsibility |
| --- | --- |
| `projects/core` | Configuration, catalogs, providers, agents, tools, runtime, chat, state and releases |
| `projects/tui` | Terminal presentation, input, navigation and image rendering |
| Versioned resources | Prompts and bundled agent/skill definitions installed alongside the executable |

## Configuration you can edit

`~/.config/duskcode/config.yml` holds provider and model selection, context budgets, limits, enabled tools, agents and skills, MCP servers and the update channel. Settings changed in the TUI are written back to this file. Themes and keybindings are currently fixed in code.

Specialist definitions live in `~/.config/duskcode/rules/agents/<name>/AGENT.md`. Their front matter controls routing, delegation links, tools and skills; the body supplies the role instructions. Change those files to adapt the team to your workflow.

| Setting | Scope |
| --- | --- |
| YAML configuration | Models, limits, integrations and enabled capabilities |
| Agent and skill files | Editable role instructions and reusable procedures |
| MCP `env` and headers | Environment variables for configured MCP child processes, and headers for configured servers |
| `DUSKCODE_GRAPHICS` | Terminal image protocol override |
| `DUSKCODE_VERSION` | Release tag selected by the installer |

MCP environment settings apply to those child processes; they are not a universal environment override for every command.

## Files stay in known locations

| Default location | Contents |
| --- | --- |
| `~/.local/share/duskcode/` | Executable in `bin/`, resources in `resources/<version>/` |
| `~/.config/duskcode/` | `config.yml`, rules, logs and session database |
| `~/Documents/duskcode/<thread-id>/` | Per-thread workspace and saved source material |

The installer links `duskcode` into `/usr/local/bin`, or `~/.local/bin` when the former is not writable. `duskcode remove` asks for confirmation before removing the installation and configuration directory.

## Telemetry and network access

Dusk has no built-in analytics or telemetry integration. That does not make a session offline: configured model providers, MCP servers and agent tools can use the network. The release updater contacts GitHub, and displaying a remote image fetches that image. Provider and external-service data policies remain separate from Dusk's behavior.

## Screenshot scenarios

The following are capture scripts, not recorded verification results. Use a configured provider with the required tools enabled, a disposable repository fixture, and a terminal with the relevant graphics support. Check the actual result before capturing it.

### Parallel repository audit

Ask:

> Review this repository without changing files. Assign three independent specialists to inspect architecture, error handling and tests in parallel. Give each a separate scope, then summarize their findings with file references and unresolved questions.

Open F3 while the agents are active. Capture the parent request together with the subagent view. This shows separate assignments and visible parallel activity; the screenshot alone does not validate the audit findings.

### A background command while the conversation continues

Ask:

> Run the following command as a background job, return its job ID immediately, and do not wait for completion: `i=1; while [ "$i" -le 30 ]; do printf 'step %s/30\n' "$i"; sleep 2; i=$((i + 1)); done`

Then ask a short unrelated question and open `/jobs` before the command finishes. Capture the reply and the running job's numbered output. This demonstrates that a background command can remain active while the agent answers another request. The fixture requires a POSIX shell and `sleep`.

### An image inside the terminal

Replace `<public-https-image-url>` with a reachable public PNG or JPEG, then ask:

> Display this image in the terminal: `<public-https-image-url>`

Capture the rendered image and the request. Record the terminal and graphics protocol used. This demonstrates remote image display in that environment; it does not demonstrate image generation or support for every image format.

### A three-step review board

Ask:

> Review the architecture of this repository without changing files. Keep a three-step task board: map the components, inspect their interfaces, and deliver a review with evidence. Update each step as the work progresses; keep unresolved questions explicit.

Capture the board while one step is complete and another is active. This shows progress tracking and task boundaries, with the final review remaining available in the conversation.

### Context and reasoning controls

After a short coding conversation, run `/context`, then `/effort high` with a model that supports the requested effort. Capture the visible context information and effort selection. The image documents the controls and selected setting; provider behavior must be verified separately.

### Local resource usage

Open the system statistics in `/settings` after starting a session. Capture the values together with the version and environment in the caption. Treat this as one observed sample, not a general memory or performance benchmark.

## Roadmap

- **Telegram continuation:** activate a Telegram bot for a selected chat and continue that session remotely.
- **GUI:** add a graphical client over the same core.

These are planned clients and workflows, not currently delivered features.

## License

[MIT](LICENSE). Use, modify and redistribute the code under its terms. Bundled skills retain their own licenses; see [resources/skills/NOTICE.md](resources/skills/NOTICE.md).
