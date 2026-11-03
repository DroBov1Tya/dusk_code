# Changelog

What changed in each release. A release's section here is also its notes on GitHub.
Patch versions are pre-releases; minor versions are full releases, made after end-to-end
testing.

## Unreleased

### Added

- /tgbot <token> mirrors the chat in a Telegram bot: what you write there goes to the
  chat, the agent's final answers come back formatted, questions come as buttons and
  approval requests as Allow and Deny, and while it works you hear every five minutes
  that the bot is alive. /stop, /resume, /status, /approve, /deny, /mode, /model,
  /compact, /goal and /alive work there, with the main ones as buttons under the
  message box. The token stays in memory; the bot answers only whoever sends it the
  pairing code duskcode shows. /tgbot alone lists the bots to stop, start or remove.

- OpenTelemetry traces: with `logging.traces` in the config, spans of turns, steps,
  model calls, tool calls and compaction go to your collector over OTLP/HTTP. Log
  lines carry the same span context.
- A logs switch in /settings (Access) and `logging.enabled` in the config: off, no log
  is written and no trace is sent, from that moment on.

- Remote MCP servers sign in with OAuth: l on one in the MCP tab of /settings opens
  its sign-in page in the browser. The tokens are kept private in the config folder
  and refreshed by themselves.

- /connect offers Anthropic and OpenAI next to a custom OpenAI-compatible server:
  pick the provider and paste an API key, the URL and name fill themselves in.
  Anthropic goes through its own Messages API, with adaptive thinking, effort and
  prompt caching. A Claude subscription cannot be used: Anthropic allows it only in
  its own apps.

- A PNG or JPEG named by its full path in a message, as a terminal pastes a dropped
  file, is copied among the chat's sources and sent to the model as a picture. Quoted
  paths, escaped spaces, `~/` and `file://` URLs work.

- Tools ask before they leave their folders: reading or writing a path outside the
  working directory and the thread workspace, and running any command. The request
  shows above the message box, subagents' too; it can be allowed once, for the session
  (that folder, or that program on plain command lines), or denied. Full access, in
  /settings, allows everything without asking and shows in the footer. The config
  folder stays closed either way.

- Install and update check every resource file against the release's tag on GitHub
  and fetch any that is missing or different, verified by its git hash. Offline, the
  archive is installed as it came.

- Subagents have phases: planning, executing, reviewing, done and paused. An agent
  moves itself on with the new `agent_state` tool in that order; each task starts in
  planning, a stop or a failure pauses it, and after a crash it is found paused. The
  sidebar and the agent window show the phase.

### Fixed

- A compaction summary no longer keeps sources the user removed, so later turns do not
  look for files that are gone. Source names go to the model as data, not as text it
  could take for instructions.

### Added

- `/connect` takes an optional name for the provider; without one it is `provider-N`
  as before. A name already in use is refused instead of replacing that provider.

### Changed

- Shipped prompts, skills and agents always follow the build: at every start, one that
  differs from the repository's version is replaced, a local edit included, and the
  start notes which. Your own agents, skills and modes, under names of their own, are
  never touched. A MAIN.md from before modes, if you wrote it, becomes the MINE mode.

- The context limit follows the model's own window when its provider reports one;
  the global input cap is used only for models that do not, and a size set under
  `/settings` > models still wins.
- A new chat's folder in `~/Documents/duskcode` is named by its day, like `03_11_2026`,
  then `03_11_2026-1` for the next one that day; its agents' folders sit inside it.
  Chats from before keep their folders.

- Every minor version is a full release and every patch version a pre-release.

## 0.70.0 - 2026-11-03

### Added

- Tool calls are checked against each tool's JSON Schema before they run; a malformed
  call comes back to the model as an error it can fix in the same turn.
- Tool calls a model writes as text (`<tool_call>` blocks) are recovered and run.
- A question's answer reaches the running turn at once instead of waiting for it to end.
- `/migrate` writes config and chats into one private archive; `duskcode restore <file>`
  unpacks it offline on another machine.
- Goals have acceptance criteria the user approves, and finish only with saved evidence.
- Effort levels `minimal` and `ultra`.
- `project-tree` skill: a compact map of a repository that is kept up to date.
- An idle screen with six animated scenes after a set time without input or work; the
  wait is set under `/settings` > appearance or `appearance.idle_screen_minutes`.
- Terminal bell when a turn ends or fails (`sound_notifications`).
- The version in the bottom-right corner; the folder name when there is no git.
- A notice in the chat when a skill is saved; copy from agent windows; resume ids shown.
- Scrolling for the task board in the sidebar.

### Changed

- New look: ink neutrals, three text greys, amber for what needs attention and a dusk
  violet for names and headings; sentence case labels, no emoji.
- Subagents run at medium effort whatever the chat's effort is.
- After a mode switch the model is told which earlier tools it no longer has.
- With many subagents the sidebar shows each in two rows, so every task stays visible.
- Up and Down in the message box go to the start or end of the text before history.
- Clicked buttons darken for a moment.

### Fixed

- A stopped subagent now reports to the agent or chat that asked it.
- `tasks` accepts steps written as objects and refuses a duplicate open task.
- Token counts in the sidebar no longer run into their labels.
- Message buttons stay on their card; Markdown tables share width fairly.
- Animations no longer replay missed frames in a burst after idle.
- Databases opened by an unreleased development build start again.
