# Changelog

What changed in each release. A release's section here is also its notes on GitHub.
Patch versions are pre-releases; minor versions are full releases, made after end-to-end
testing.

## Unreleased

### Added

- Project memory: switched on in /settings > personalize, next to your own memory, the
  agent offers to keep a short summary of the project in the working directory and then
  keeps it up to date itself. It lives on this machine in
  `~/Documents/duskcode/projects/`, never in the repository, with its own size limit.
- The idle screen plays eleven new scenes in place of the six: a mountain lake and a
  river at dawn, a desert camp under the Milky Way, dunes, a campfire, a caravan at
  dusk, a knight resting by his fire, a lantern festival, northern lights over a cabin,
  a neon sunset and the sea at sunset. They are drawn in dense coloured halftone dots,
  eight to a cell, and fit the whole window at any size, offline.

- The Telegram bot lists its commands in Telegram's menu as soon as it is added. /mode
  and /model without a name answer with buttons to pick from, or, past ten options, a
  list to send `/model <name>` from.

- /connect opens on a list of providers: OpenAI, Anthropic, Ollama on this machine and
  any OpenAI-compatible server; picking one opens a form of only what it needs, with
  its name and URL filled in. For OpenAI and Anthropic, ctrl+o opens the page where a
  key is made.

- Computer use: a screenshot can be of one app's window, found by its app name or
  title, even under other windows; clicks then land in that window. A ring of dusk's own
  marks where the agent last clicked or scrolled; it lets the mouse through and stays
  out of screenshots.

### Changed

- The token counters are gone from the sidebar, and the input and output token limits
  from /settings and the config. A request
  is held to the model's window, or to the size set under /settings > models, and a
  model writes up to its own output cap. Older configs that set them still load.

## 0.70.1 - 2026-11-03

### Added

- Anthropic and OpenAI in /connect next to a custom OpenAI-compatible server: pick one,
  paste an API key. Anthropic uses its Messages API with adaptive thinking and prompt
  caching. `/connect` also takes a name for the provider.
- Approvals: reading or writing outside the working folders and running commands ask
  first, once or for the session. Full access in /settings allows everything.
- Computer use on macOS: screenshot, click, type_text, press_keys and scroll, off by
  default, for models that see pictures. The screen is asked for once a session,
  typing and keys every time.
- A picture named by its path in a message goes to the model as an image.
- /tgbot mirrors the chat in a Telegram bot paired by a code, with buttons for
  questions and approvals.
- Remote MCP servers sign in with OAuth from the MCP tab of /settings.
- OpenTelemetry traces over OTLP/HTTP (`logging.traces`) and a switch that turns logs
  off.
- Subagent phases: planning, executing, reviewing, done and paused, shown in the
  sidebar.
- Install and update fetch any resource file that is missing or differs from the
  release, checked by its git hash.

### Changed

- Shipped prompts, skills and agents follow the build at every start; your own, under
  their own names, are never touched.
- Agents and skills that link to ones not installed still load; the missing links are
  shown as notes.
- The context limit follows the model's window when the provider reports it.
- Errors for invalid, unpaid or unknown keys and models say what to fix.
- A new chat's folder is named by its day, like `03_11_2026`.

### Fixed

- A compaction summary no longer keeps sources the user removed.

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
