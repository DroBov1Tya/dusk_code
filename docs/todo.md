# Todo

The working list after 0.71.1 (user requests, 2026-11-10). GUI-only history and the
framework notes live in `gui-todo.md`. Rules: the client only draws and calls the core;
TUI and GUI stay at parity; no release, tag or version bump without the user's order.

Marks: `[x]` done, `[~]` core done or partly done, `[ ]` open.

## Agent engine (core)

- [x] Study why opencode 2.0 beats 1.19 on benchmarks (see `agent-engine.md`)
- [ ] Work through the plan in `agent-engine.md`
- [x] Files are written, edited and deleted through `run_command`; the sandbox confines
      it to the same folders, and a skill saved by a command still gets its notice
- [ ] Per-chat list of files a chat changed (the GUI Outputs module went with the
      write tools); could come from the core comparing `git status` around a command
- [ ] Analyze how to make the agent faster without losing quality
- [ ] Prompt caching: stable prefix (system, tools, memory) first, cache breakpoints,
      count cache reads; spend fewer input tokens
- [x] Retries for failed requests without a clear error code (backoff, capped)
- [x] Client-side token bucket per provider: `requests_per_minute` in its config
- [x] A mode switch applies from the next turn: the tools at once, the mode's prompt
      leads the next request
- [ ] A command or agent code already running can be moved to the background by hand
      (TUI + GUI)
- [ ] `run_command` runs in the background: the chat stays writable while it works;
      a running command can be moved to the background (TUI + GUI)
- [ ] MCP code mode: the model writes one script that calls a set of MCP tools
- [ ] Subagent answers are their own step on the agent's side (not the user's message),
      foldable, outside the thinking/tools fold
- [x] `name_session` tool: named from the first user message, short enough for the list
- [x] `plan_edit` tool for the pinned plan
- [x] Output tokens per second per agent (core events)
- [x] Context report per item with usage counts
- [~] Per-thread access mode (core done; UI below)
- [~] Version control settings: branch prefix and the like, named "version control",
      not "git" (core done; UI below)
- [~] Notifications and their settings (core toggles done; delivery and UI below)

## User context

- [ ] User profile (who I am, links to files such as a CV), each file with a "send"
      flag, called with `@me` (TUI + GUI)
- [ ] User `.md` snippets called as `@name` (e.g. `@video_1` with a script): a prompt
      injection the user writes on purpose
- [ ] Pinned plan agreed between the user and the model: markdown, opened in the app,
      a view and an edit mode, on the right (GUI) and a command (TUI)

## Providers and voice

- [x] Cost of tokens on paid APIs, never guessed: the cost the provider reports
      (OpenRouter `usage.cost`, xAI `cost_in_usd_ticks`), else the `prices` the user
      writes per model in the provider's config; otherwise unknown. Stored per call in
      micro-USD with its source; cache writes counted apart
- [ ] Prices from the provider's pricing API (OpenRouter `/models`, LiteLLM
      `/model/info`) and LiteLLM's cost header, so fewer models need a hand-written price
- [x] Balance where a plain key may read it, sent only to the provider's own host:
      DeepSeek, Kimi and OpenRouter by host, LiteLLM with `balance: litellm`
- [x] DeepSeek cache hits counted (`prompt_cache_hit_tokens`)

- [ ] Several providers at once in Models: add, use, remove
- [ ] STT (microphone button by send) and TTS (speaker button by a model answer)

## GUI

Chat
- [x] A command block with a copy button also gets Run: the core runs it in the chat's
      folder after a confirm, the output waits in the composer
- [x] Code blocks drawn as markdown code in the chat, copy button on each answer
- [x] Select text in messages
- [x] Click a picture to open it large over the window; a click outside closes it
- [x] Only the agent's last action shows; a click unfolds the rest, each unfolds again
- [x] Model screenshots go to a menu on the right, not into the chat
- [x] A large paste becomes an attached file, as in the TUI
- [x] Attachments above the composer, not under it
- [x] Send and + buttons a little larger
- [x] Under the composer: access mode (sandbox, arbiter, full), per chat, the setting
      is the default; review that row so it is not crowded; mode shown, Tab switches
- [x] Messages typed while the agent works wait above the composer, editable; they go
      as one message when the turn ends (as in the TUI), "Now" interrupts
- [x] Tokens per second shown while the agent writes
- [x] Message navigator: opaque

Subagents
- [ ] A button right of the composer with a status indicator; it opens a window with
      subagents grouped by role, live state (thinking, tool...) with a small 8-bit
      mascot animation per state
- [ ] Names and run time on subagent rows; shorter rows
- [ ] Richer generated figures (advanced math curves; keep pentagrams)

Arbiter
- [ ] One arbiter window per chat, not shared
- [ ] Its scroll does not move the chat behind it
- [ ] Much slower rotation; on each request a "resonant sphere": a compression wave from
      the center, points pull in as it passes and return, the whole mass breathing

Left column
- [ ] "duskcode" at the top left: "dusk" in a violet to sunset orange gradient
- [ ] A working animation on chats where an agent runs
- [ ] Right click menu: highlight the hovered item; menus near the bottom open upward
- [ ] Delete button must not click through to the chat under it
- [ ] A short fade when a chat is deleted
- [ ] Delete all artifacts at once (confirmed)

Right column
- [ ] Activity and Context inside one "Statistics" parent that folds as one
- [ ] Environment path: a click opens a menu: Show in Finder, Open repo, Open in
      terminal, Copy path
- [ ] Pinned plan module (see User context)
- [ ] Less information at once overall: calmer, fewer simultaneous things

Settings and updates
- [x] App icon on a bare binary (Dock on macOS, app ID on Linux)
- [ ] Notifications section
- [ ] Version control section (branch prefix ...)
- [ ] "new release!" label when an update exists; a dev update channel switch

Motion
- [ ] Animations that answer a user action are under 400 ms; purely visual ones may be
      slow and beautiful

From `gui-todo.md`
- [ ] Core: an `icon` field on McpInfo
- [ ] Model menu: close on a click outside, keys 1-6 pick a model
- [ ] Linux build in OrbStack
- [ ] Pasted screenshots saved to the project's attachments folder
- [ ] Context details on click (core ready)
- [ ] Drag a chat to split the center in two
- [ ] Project environment variables (secrets, never logged)

## TUI

- [x] "done" sometimes does not fit (subagent rows past an hour, long tool lines)
- [x] With effort off, do not print "thinking" for each action; show tps
- [x] A typed own answer to a question wraps long lines
- [ ] Parity: `@me` and `@name`, background commands, access mode per chat, pinned plan

## Repository

- [ ] Remove `resources/internal/RETIRED_SKILL_FILES.md`: blocked, the 0.70.2 updater
      checks for it before updating (`release/shipped.rs`); ask the user when to drop 0.70.2
