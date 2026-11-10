# Changelog

What changed in each release. A release's section here is also its notes on GitHub.
Patch versions are pre-releases; minor versions are full releases, made after end-to-end
testing.

## Unreleased

### Changed

- GUI: code in the file viewer had the library's light-theme colors, so names were dark blue
  on a dark page; each theme now has its own syntax colors from its palette.
- GUI: the repository was read only while the Changes module was open, so a folded one left the
  footer saying "no repository" and hid the module; it is read whenever a project is chosen.
- GUI: the tint of added and removed lines in the review spans the whole row.
- GUI polish: tooltips on every icon-only button, counts in the singular when they are one
  ("1 tool", "1 line"), the approval mode shown by name ("full access", not "fullaccess"), and
  the composer menus and the message list fade in over 140 ms.
- Compaction sends the thread's tools, project text and cache key, so its request reads the
  cached prefix instead of the whole context cold.
- The log has a `model call` line with prompt, cached, written and completion tokens, and a
  `prompt cache dropped` warning when less than half of the last prompt came from the cache.
- No count limits on tools: a turn is no longer cut after 150 steps, one answer may call any
  number of tools, and an MCP server's tools are no longer cut at 128. The user's stop and
  context compaction end a long run.

### Added

- `--features screenshot` for the GUI (development only): the window follows a script and
  saves PNG pictures of its pages, drawn in memory. See docs/gui-screenshots.md. It makes the
  lockfile list 26 more crates (gpui's test support); none is built without the feature, and
  `cargo audit` shows no new advisory.
- GUI: a path in a code span of a chat message, such as `src/main.rs:42`, opens the file
  viewer at that line; only relative paths in file-name characters are linked. On the review
  page Up and Down choose the file and Escape goes back.
- GUI: a file viewer in the middle column, read-only, with line numbers, syntax colors,
  folding and search. Opened from the Files module and from the review page. It adds the
  `syntect` highlighter of the UI kit (new crates: syntect, two-face, fancy-regex, bincode;
  `cargo audit` lists bincode as unmaintained, no new vulnerability).
- Review of changes: the Changes module shows the lines added and removed, and opens a page
  with each file's diff (line numbers, green, red and grey lines). TUI: `/diff`. The core
  answers `Diff` for a project and for a chat's own folder, with the totals in `GitInfo`.
- A call repeated three times with the same result gets a note to change course; it stands
  in for the removed step cap.
- `duskcode run "prompt"`: one prompt in a new chat, the answer on stdout, progress on stderr.
  `--json` prints the answer with tokens, calls and tool counts; `--access`, `--project`,
  `--timeout` and `-` for a prompt from stdin. Approvals are denied unless `--access`
  allows them, questions get a note that nobody is there; exit 1 on failure or timeout.
- GUI: what a subagent reports back is a folded card on the agent's side of the chat,
  with its name and first line, instead of a bubble that looked like the user's message.
- The files a chat's commands changed in the project are kept per chat: the core compares
  git's list of changes around each foreground command. GUI: a "Touched" module in the
  right column; TUI: `/files`.
- TUI: `/me` edits what `@me` says about you and `/snippet name` writes a snippet for
  `@name`, in the same editor as pasted text; empty text removes a snippet.

- Providers: the GUI's Model page lists the connected providers, each with what it
  answered and a Remove button, and connects another from a form; the TUI has
  `/disconnect name`. The provider of the chosen model stays until another model is
  chosen.

- Notifications reach the system's own area: the TUI and the GUI post a notice when
  a chat finishes, fails, asks a question or waits for approval and the terminal or
  window is not in front, with the sound where it is on (macOS Notification Center,
  `notify-send` on Linux). The TUI now tracks whether its terminal is in front.

- GUI: the right column is calmer. At first start only the plan, the todo and the
  agents are open, a folded module shows a few words of its state in its header, and
  which modules are folded is kept between runs.

- GUI: the options of a question from the agent are numbered cards, the chosen one
  outlined.

- Project variables: the GUI project form sets environment variables, such as API
  tokens, for the commands that run in the project's folder, foreground or in the
  background. Values are write-only, kept in the core's database, and masked in
  everything a command prints, also at the point where long output is cut. Names that
  change how programs load (`PATH`, `LD_*`, `DYLD_*`, `BASH_ENV`, `IFS`, `SHELL`, `ENV`,
  `NODE_OPTIONS`) are refused. On Linux the sandbox cannot yet hide the database from a
  command.

- The chat's pinned plan is shared: the model keeps it with `plan_edit`, and the user
  reads and rewrites the same Markdown in the GUI's Plan module (also from the + menu)
  or with `/plan` in the TUI. It comes back with every message; saving empty text
  removes it.

- TUI: `/access` sets the access mode of the chat (ask, sandbox, arbiter, full, or back
  to the default), and a "version control" page in the settings edits the branch prefix.
- GUI: the name in the sidebar reads Duskcode.

### Fixed

- GUI: menus and lists that open over the page (the message navigator, the model,
  mode, access and attach menus, the mention list, the folder menu) are painted above
  everything, so the text and buttons beside them no longer show through, and they
  stop the mouse from reaching what is under them.

## 0.71.2 - 2026-11-10

### Added

- GUI: system notifications when a chat finishes, fails or asks while the window is in
  the background; a pasted screenshot becomes an attachment; a part of the context
  unfolds to what it is made of; digits 1-6 pick a model in the model menu.
- MCP servers take an `icon:`, a picture in `~/.config/duskcode/icons/`, shown on their
  card.
- `@me` and `@name` in a message: the profile (who you are, files marked to send) and
  the user's snippets become the chat's sources when it is sent; `@` completes in the
  TUI and GUI, and GUI Settings > About me edits both.
- macOS releases carry the desktop client as Duskcode.app in a disk image, with the
  resources of its version inside the bundle, so it runs on a Mac with nothing else
  installed (`scripts/package-macos.sh`, signed ad hoc).
- The desktop client keeps messages written while the agent works above the box and
  sends them when the turn ends; it shows tokens per second and its icon on a bare
  binary.
- The cost of a chat, as the provider reports it or from a `prices` list per model in
  the provider's config, shown next to the call count; never guessed. Providers that
  tell the balance to a plain key (DeepSeek, Kimi, OpenRouter, and LiteLLM with
  `balance: litellm`) show it in the model list.
- Desktop columns and settings: the duskcode name at the top, a breathing dot on
  chats at work, a fade when a chat is deleted, a chat menu that opens upward near
  the bottom and never clicks through, delete all artifacts shown, Statistics with
  context and activity, a folder menu (show, open repo, terminal, copy path),
  Notifications and Version control pages, a dev update switch and a "new release!"
  label, and a round button that scrolls the chat to its newest message.
- Desktop subagents: a button by the message box with a lit dot while any works opens
  them by role, each with a small 8-bit figure that thinks, walks, hammers, rests or
  fails; rows show name, role, state and run time on two lines; marks add
  spirographs, Maurer roses and superformula shapes. The arbiter window belongs to
  its chat, turns slowly and sends a resonant wave through its sphere at each request
  and answer; no window over the app passes the mouse or the wheel to what is behind.
- A command the agent runs can be sent to the background while it runs (Ctrl+B in the
  terminal, "To background" in the desktop client): the agent goes on and gets the
  result when the command ends. Background jobs now keep the start and the end of
  their output.
- Desktop chat: Copy and Run on code blocks (Run asks, runs in the chat's folder as
  its tools would and puts the output in the message box), a copy button on answers,
  selectable messages, pictures that open large, screenshots in a right column module,
  long pastes as chips kept as files, attachments above the box, and the chat's own
  access mode under it.
- Per-chat access modes, version control and notification settings, name_session and
  plan_edit tools, a per-item context breakdown with use counts (core).

### Changed

- Agents spend fewer steps and tokens: files are read a window at a time, commands keep
  the end of their output, subagents no longer report phases by hand, and the prompts
  ask for more work per step.
- `write_file`, `edit_file` and `delete_file` are gone: files change through
  `run_command`, which the sandbox confines to the same folders. A skill saved by a
  command still gets its notice. The desktop client's per-chat Outputs module went with
  them.
- `run_command` keeps the folder a command ended in for the next call (back to the
  project when it left it), takes the user's shell PATH, so an app started from the Dock
  finds cargo and the rest, waits 120 seconds by default, and takes a short description
  the clients show.
- Requests that fail without a clear cause are tried again, up to six times, with
  growing pauses and the provider's retry-after.
- DeepSeek cache hits are counted.
- Agents think once, then act: the first step of a turn and a step after a failed
  tool reason at the effort set, the steps that carry out the plan at a light effort,
  and the prompts ask for one plan and then tool calls, reasoning again only when a
  result surprises.
- A mode switch applies from the next turn with that mode's prompt, not only after a
  compaction. OpenAI requests carry a cache key per thread. A provider can be held to
  `requests_per_minute`.
- The terminal client keeps a subagent's state and a tool's status in view on narrow
  rows, wraps a long typed answer to a question, and with effort off says "reading"
  with the pace in tokens per second.

## 0.71.1 - 2026-11-05

### Added

- The desktop client's arbiter window: the chat's agents, what passed between them,
  the approval requests and the arbiter's answers, around a sphere that stirs at each
  answer.
- Subagent chats open to read, live while they work; pictures show as cards with
  their title and description; the version in the corner opens the updates.

### Changed

- Long chats scroll smoothly: only the messages in sight are drawn.
- The message navigator is four ticks by the composer and opens over the chat.
- The sidebar folds into a narrow rail of icons.

## 0.71.0 - 2026-11-05

### Added

- The desktop client, duskcode-gui, a binary of its own, so a server installs only the
  terminal client. Projects with their chats, pinned and recent chats, search, chat
  rename and delete; the chat with markdown, links, pictures and media cards, folded
  agent steps, approvals and questions, attachments and a message navigator; model,
  effort and mode menus with a context ring; connectors with details and per-tool
  switches for MCP servers, skills, plugins, built-in tools and the agents by level;
  artifacts by kind with a viewer; the chat's environment, context, activity, outputs,
  changes, files, todo, agents and jobs on the right; settings with themes, fonts,
  limits, updates and system usage.
- Projects in the core: named folders with their git state and files, shared by
  every client; chats can be renamed and deleted for good.
- install.sh asks for the terminal client, the desktop client or both, and
  scripts/install-gui.sh installs or updates the desktop client.

### Changed

- A provider may use plain HTTP to a private network address, and to any host with
  `allow_http: true`.
- A model the provider does not list can be chosen by name, also with
  `/model provider/name`.
- An older build opens a database a newer build added tables to.

## 0.70.6 - 2026-11-04

### Added

- Computer use on macOS can work in one app's window: the window is chosen once, its
  screenshots and clicks stay inside it even when another app covers it, and the
  window list, capture and control are approved apart.
- A click in a window session shows the agent's cursor where it presses.

### Fixed

- A revoked Accessibility permission is asked for again, and a click that finds no
  control inside the app's visible window retries before it fails, with a clearer error.

### Changed

- More of the code is grouped around types with methods: the archive code in one
  folder, the input, transcript and settings views, and the shipped-file sync. Behaviour
  is the same.

## 0.70.5 - 2026-11-04

### Added

- A plugin's secret settings, such as the Telegram token, are entered in `/plugins` with
  s, in a masked field. They are kept in memory for the session and never reach the
  config file.
- The idle scenes are drawn with block, braille or ascii glyphs instead of one dot per
  cell, which is finer on a terminal. The aurora cabin is larger and the knight's woods
  have leafy crowns.

### Fixed

- Unpacking an update no longer depends on the archive's file times, and when `tar`
  fails its message is shown.

### Changed

- The code is regrouped around types with methods: one file per computer tool, a
  `Desktop` type for the macOS and Linux backends, `Modes`, `ThreadSources`, `Sidebar`
  and a separate mouse module. Behaviour is the same.

## 0.70.4 - 2026-11-04

### Fixed

- 0.70.2 could not update to 0.70.3, as it wanted a resource file 0.70.3 dropped. The
  file is back for it, and an update no longer holds a newer version to the running
  build's list of files.
- y copies what the update window says, an error included.

## 0.70.3 - 2026-11-04

### Added

- Computer use on Linux: X11 through xdotool with maim, scrot or import, Wayland through
  ydotool with grim, gnome-screenshot or spectacle. A missing program is named.
- On macOS a click presses the control through Accessibility and leaves the user's
  pointer alone; clicking with the real pointer, and scrolling, take mode system and a
  yes of their own.
- The agent's cursor glides to each point and hides after ten seconds at rest.
- `display_image` shows a file or an earlier screenshot as well as a public URL, and
  the Telegram plugin sends such cards to the owner as photos.
- Hovering a message shows when it came, under its buttons.
- A goal's proposed criteria are approved or sent back from buttons, in the TUI and in
  Telegram, instead of `/goal approve` with a revision copied by hand.

### Changed

- The goal bar shows approve and change for a draft, resume only for a paused goal.
- The core's state and providers are grouped into folders by what they hold.
- The idle scenes draw dots of three sizes: fine ones in the dark, larger toward lights
  and highlights.
- Skill helper files older builds shipped are removed when unchanged, and the
  `retired-skills` folder they were moved to goes with them.

### Fixed

- The shipped Telegram plugin's `plugin.yml` failed to parse.
- The Tasks list jumped back to its top while the agent worked.

- An MCP server that stops after it was ready shows as failed instead of ready, and its
  tools leave new threads until it is restarted from /settings.
- A tool call some models write inline with its arguments as a string is read like the
  other inline calls instead of failing.

## 0.70.2 - 2026-11-03

### Added

- Plugins: a folder with a plugin.yml under `~/.config/duskcode/rules/plugins/` brings
  skills, agents, MCP tools, providers, hooks on thread events and platforms such as a
  chat bot. They are found while duskcode runs and stay off until switched on in
  /plugins; docs/plugins.md describes them.
- Sandbox mode in /settings > access: commands run without asking, writing only in the
  working folder, the thread's workspace and temporary folders, with the config folder
  closed (Seatbelt on macOS, Landlock on Linux, which keeps writes out but not reads).
- Arbiter mode in /settings > access: an agent decides approval requests in your place
  and refuses what is destructive, outward or unclear. Each verdict shows in the chat;
  the arbiter's card in the sidebar opens its thread.
- Project memory in /settings > personalize: the agent offers to keep a short summary of
  the project and maintains it, in `~/Documents/duskcode/projects/`, never in the
  repository.
- /connect starts with a list of providers, Ollama included; each opens a form of only
  what it needs, and ctrl+o opens the page where OpenAI or Anthropic keys are made.
- Computer use can capture one app's window, even under others, and shows a cursor of
  its own where the agent clicks: an arrow with a glowing rim that rocks slightly,
  passes the mouse through and stays out of screenshots.
- `ask_agent` takes an `effort` for the subagent; its window shows the level it works at.
- The Telegram bot sets its command menu when added, and /mode without a name answers
  with buttons.
- Eleven new idle scenes in dense coloured halftone dots, a knight by his fire among
  them; they fit the window at any size.

### Changed

- The Telegram bot is the built-in `telegram` plugin: its token goes under its settings
  in config.yml, it runs whether or not a chat is open, and its pairing outlives a
  restart. /plugins replaces /tgbot.
- The token counters left the sidebar and the input and output token limits left the
  config: requests follow the model's window or the size set under /settings > models,
  and answers the model's own output cap. Older configs still load.
- skill-creator reviews its runs in Markdown instead of a browser page.
- spike, systematic-debugging and test-driven-development are rewritten as dusk_code's
  own skills.
- A subagent past `max_agents_at_work` is refused instead of queued, so the asker knows
  it is not at work.

### Fixed

- The update check runs every half hour instead of every minute, so it no longer uses up
  GitHub's request limit.
- A binary installed without its resources rebuilds them from the release on GitHub. It
  fetches only files missing or changed since the installed snapshot, shows its progress
  before the screen opens, and fails at once when the release lacks a file the build
  needs.
- Subagent cards in the sidebar are parted by a blank row when many are listed.
- Scrolled up while the agent writes, the transcript stays on what is being read;
  within three lines of the bottom it still follows the output.
- Computer use asks macOS for Accessibility, so the terminal shows up in its list to be
  switched on.
- A provider that does not answer no longer holds /model or the model search at start.
- Two writes to one memory or agent file at once no longer share a staging file.
- An empty answer after a long think is asked again instead of failing the turn, and a
  turn may take 150 tool steps instead of 50.

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
