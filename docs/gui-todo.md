# GUI: plan, structure and handoff

Branch: `gui`. Binary `duskcode-gui` in `projects/gui` (crate `dusk_gui`). Platforms:
macOS and Linux. Read this file first; it is enough to continue without the chat.

## Rules (from the user, do not break)

- The client ONLY draws and calls the core. All logic that changes state lives in
  `projects/core` and is reached through `projects/protocol` (`Request`/`Response`,
  `Event`, `Status`). TUI and GUI must stay at parity: a new core feature is a common API.
- Presentation-only things (themes, icons, layout, which tab is open) stay in the GUI.
- Modular files: each file is named for what it does; no huge files.
- Security and few bugs first; v1 does not need to be perfect.
- Comments, logs and errors in English, short, no emoji. Plain `pub`. Only critical tests.
- Git: branch per feature, `--no-ff` merge into main, no co-authors in commits.
  Never tag, bump a version or release without the user's direct order.
- Work in the main checkout, not `.claude/worktrees`.

## Framework decision

`gpui-kit = "=0.9.0"` (GPUI + gpui-component, Zed's GPU renderer; Metal/Vulkan).
Chosen over iced and Tauri for the best graphics, response and components (dock,
sidebar, tree, tabs, markdown `TextView`, inputs, theme system), with one process and
no webview. API is unstable: keep the version pinned.

Useful API facts (checked in `~/.cargo/registry/src/*/gpui-{kit,component,base}-0.9.0`):
- `gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| ..)`,
  `gpui_kit::init(cx)`, `gpui_kit::open_window(options, cx, |window, cx| cx.new(..))`.
- Theme: `component::theme::{Theme, ThemeConfig}`; build a `ThemeConfig` from JSON
  (keys like `"sidebar.background"`, see `default-theme.json`), then
  `Theme::update(cx, |t| t.apply_config(&Rc::new(config)))` and `Theme::change(mode, None, cx)`.
  Colors: `cx.theme().background`, `.foreground`, `.border`, `.muted_foreground`,
  `.primary`, `.sidebar`, `.accent`, `.success`, `.warning`, `.danger`...
- Inputs: `component::input::{InputState, TextareaState, InputEvent}`; `Input::new(&state)`,
  `Textarea::new(&state)`; `TextareaState::new(window, cx).auto_grow(1, 8)`,
  `.placeholder(..)`, `set_submit_on_enter(true, cx)`; `InputEvent::PressEnter { secondary, shift }`;
  subscribe with `cx.subscribe_in(&state, window, |this, _, event, window, cx| ..)`.
- Markdown: `TextView::markdown(id, text)` (`component::text`).
- Button: `component::button::Button::new(id).label(..).icon(..).ghost().on_click(..)`.
- Geometric icons: `canvas(prepaint, paint)` + `PathBuilder::stroke(px(1.5))`,
  `move_to`, `line_to`, `build()`, `window.paint_path(path, color)`.

## Build note

gpui needs a newer Rust than 1.95 (it uses `Option::map_or_default`). Build the GUI
with `cargo +1.99.1 build -p dusk_gui` or update stable (`rustup update stable`).
The TUI still builds on 1.95.

## Done

1. Core API `project` (commit `2fe96d7` on `gui`), protocol version 2:
   - `protocol/src/project.rs`: `Project`, `GitInfo`, `Worktree`, `Change`, `ChangeKind`,
     `FileEntry`, limits `MAX_NAME_CHARS`, `MAX_DESCRIPTION_CHARS`.
   - `Request::Project(ProjectRequest::{List, Save, Remove, Git, SwitchBranch, Files})`,
     responses `Projects`, `Project`, `Git`, `Files`.
   - `core/src/project/{mod,git,files}.rs`: folder is canonicalized on save; files and git
     work only inside a saved project; `inside()` refuses absolute paths, `..` and
     symlinks out; git runs without a shell, with fixed args, `GIT_TERMINAL_PROMPT=0`,
     a 10 s timeout; a branch switch needs a known local branch and a clean tree.
   - `core/src/state/projects.rs` + migration `0020_projects.sql` (dir is UNIQUE).
   - `client.projects()` handle in `core/src/client/mod.rs`.
   - Chats belong to a project by `Thread.project` (the folder they started in, existing
     `thread_projects` table). No schema change for chats.
2. `Client::shutdown(&self)` now borrows (so the GUI can share the client in an `Arc`).
3. GUI scaffold builds; started: `main.rs`, `core.rs` (bridge), `theme/` (6 palettes).

## Planned structure of `projects/gui/src`

```text
main.rs              start tokio + core, open the window, shut the core down on quit
core.rs              Core: Arc<Client> + tokio runtime; call(cx, |client| fut, |view, result, cx| ..)
workspace.rs         Workspace: the one root view; holds the state below, routes core events,
                     renders the three columns and the center tabs (Chat | Settings)
widgets.rs           small shared pieces: section header, badge, empty state, icon button
theme/mod.rs         apply a palette, remember the choice in <config>/gui-theme
theme/palettes.rs    Dusk (default), Light, Dark, Solar Dusk, Eclipse, Lotus
sidebar/mod.rs       left column: connectors button, search, projects with chats, recent
sidebar/projects.rs  project rows (collapsible), add/edit project form (name, description, dir)
sidebar/threads.rs   chat rows of a project, new chat button
sidebar/recent.rs    every chat by last activity, with its source badge (tui, gui, telegram...)
sidebar/search.rs    filter chats by title
chat/mod.rs          Chat state: open threads (map thread_id -> Transcript), the shown one
chat/transcript.rs   Transcript items from history (Message) and live ThreadEvent; render
chat/composer.rs     textarea, Enter sends, Shift+Enter new line, stop button while working
chat/model_bar.rs    under the composer: model and effort buttons, context meter
chat/model_menu.rs   popover over the bar: models with a check, Effort and More models submenus
chat/messages.rs     the transcript drawn: markdown answers, folded reasoning, tool lines
chat/approval.rs     approval card (Once / Session / Deny) and question card (options)
inspector/mod.rs     right column tabs + terminal button
inspector/git.rs     branch, ahead/behind, changes, branch switch, worktrees
inspector/files.rs   lazy file tree with git change colors
inspector/context.rs ContextReport sections
inspector/todo.rs    the thread's tasks
inspector/agents.rs  subagents with generated icons, their state
inspector/jobs.rs    background jobs, output, stop
inspector/glyph.rs   deterministic geometric icon (spiral, rose, polygon star) from fnv1a(thread_id)
connectors/mod.rs    center page with tabs Connectors (MCP), Skills, Plugins, Tools, Agents + search
connectors/card.rs   card: icon slot (letter tile until the core gives an icon), name, kind, state, action
connectors/{mcp,skills,plugins,tools}.rs  one tab each, two-column card grid
connectors/graph.rs  agents graph: parent left of children, bezier edges, dashed edges for links
settings/mod.rs      settings tab: theme, approvals, mode, memory, work history, limits, logging
host/terminal.rs     opens the system terminal app in the project folder
                     (macOS: `open -a Terminal <dir>`; Linux: $TERMINAL, x-terminal-emulator,
                     gnome-terminal --working-directory, konsole --workdir), no shell
```

Layout: left = settings, chats, connectors; center = chat (or settings tab); right =
engineering: git, files, context, todo, agents, jobs (later: browser, screenshots,
watched windows, cron).

## Core calls the GUI uses (all exist)

- Threads: `client.threads().list/start(cwd)/resume(id, cwd)/messages/tasks/send(id, text, now)/cancel/close/approve(id, Decision)`.
  Live events: `Event::Thread { thread_id, event: Box<ThreadEvent> }`, `Event::Ended`.
- Model: `client.command(Command::Model { name: None })` -> `Answer::Providers`;
  switch with `name: Some("provider/model")`. Effort: `Command::Effort { level }`.
  Context: `Command::Context { thread_id }` -> `Answer::Context(ContextReport)`.
  Settings data: `Command::Settings` -> `Answer::Settings(Box<Settings>)`.
- Switches: `client.settings().switch(Switch::{Skill, Mcp, Agent, Tool, Memory, WorkHistory, Logging, Approvals}, on)`,
  `plugin_enabled`, `plugin_secret` (memory only, never saved), `next_mode`/`mode`.
- MCP: `client.catalog().restart_mcp/sign_in_mcp`.
- Jobs: `client.work().jobs/job_output/stop_job`.
- Projects: `client.projects().list/save/remove/git/switch_branch/files`.
- Status (`client.status`, watch): model, effort, input_limit, mode, approvals, mcp, plugins, update.
- Question answer: `question.answer(&chosen, own)` -> send as a message.

## TODO v1 (in order)

- [x] Core: projects, git, files API with tests
- [x] GUI crate scaffold, bridge, themes
- [x] workspace.rs: three columns, event pump (`events.recv()` in `cx.spawn`), status watch
- [x] sidebar: projects (add/edit/remove), chats per project, collapse, search, recent with source
- [x] chat: transcript (markdown, reasoning collapsed, tool rows), composer, cancel
- [x] chat: approvals and questions
- [x] model bar: model, effort, context meter
- [x] inspector: git (switch branch, worktrees), files with git marks, context, todo, agents with glyphs, jobs
- [x] connectors: MCP / skills / plugins / agents switches
- [x] settings tab with the theme picker
- [x] terminal button
- [x] cargo clippy --all-targets clean for dusk_gui, cargo test (core 178, gui 1)
- [x] connectors as a center page with cards, icon slots and a Tools tab; agents as a graph
- [ ] core: an `icon` field on McpInfo (URL or file under the config) for enterprise connectors
- [ ] check every panel by hand in the running app (the user reports bugs)
- [ ] model menu: close on a click outside (`on_mouse_down_out`), keys 1-6 pick a model
- [ ] Linux build in OrbStack (see memory `linux-checks`): needs Vulkan/X11/Wayland libs

## Round 2 (user requests, 2026-11-05)

Design rules (from heydamir/unslop, CC BY 4.0, credit Damir Sokolovsky): neutral
surfaces tinted only slightly toward the theme's world; one accent used rarely
(selection, primary action, one key figure); hairline borders = primary text at 6-10%;
no glows, gradients or shadows on dark; 4px grid; nav rows 32-36 tall, 2-4 apart;
section labels 24 above / 8 below, sentence case (no uppercase tracking); three text
roles (primary, secondary, muted). The user stares at this for hours: calm, uncluttered.

- [x] Dusk theme in the dark fantasy of Berserk / Dark Souls 1 (ash, iron, worn leather,
      bonfire ember accent, parchment text), not evil or neon; tune every palette
- [x] Larger default text; settings for a system font family and sizes (UI and mono)
- [x] Settings as a page with its own category menu on the left (Appearance, Model,
      Chat, Safety, Memory and context, Advanced...), like the Hermes settings screenshot
- [x] Left: project rows not highlighted by default, hover only, a tone different from
      chats, more space between a project and its chats
- [x] Search opens a modal over the window, with recent chats under the field
- [x] Artifacts tab on the left: files by category (text, images, documents such as
      docx/pdf/csv, other) with a preview; needs a core API that lists the workspace
      and artifacts folders and reads a file (size-capped, inside those roots only)
- [x] New project as a large modal in the middle; a Browse button opens the system
      folder picker (`cx.prompt_for_paths`); description is a real multi-line field
- [x] A new chat without a project runs in the workspace root (~/Documents/duskcode)
- [x] Effort lives inside the model menu only (no separate button)
- [~] Chat rendering, high end (done: markdown with tables via TextView, http(s) links and images, checklist questions; left: option cards, document preview in chat): full markdown (tables, lists, code, quotes), clickable
      links (open in the system browser, http/https only), images in answers (https
      only), option lists rendered as cards, multi-choice questions as a checklist with
      a submit button (like the "Выбери направления" screenshot), a small document
      preview for outputs
- [x] Right column as stacked collapsible modules (Environment, Context, Git, Files,
      Todo, Agents, Jobs, Outputs), like the TUI sidebar, not tabs
- [x] Outputs module: files the agent created or changed in the chat; open one in a
      built-in viewer with markdown rendering
- [x] Context module: a fill bar with each kind's share in percent
- [x] App icon (placeholder) under projects/gui/assets; macOS .app bundle in the installer
- [x] scripts: install-gui.sh (detects an existing install, updates or installs),
      install-tui.sh, and a general install.sh that offers TUI, GUI or both and calls them;
      README section

Also done in round 2: HTTP to private networks for providers (any host with
`allow_http: true`), unlisted models by name (model menu and Settings > Model), older
builds open newer databases (`ignore_missing` on the migrator).

## Round 3 (user requests, 2026-11-05), version 0.71.0 local, release with GUI in 0.72

Layout rule: the left column is shared; the right column belongs to a chat (with a
split view each chat half gets its own right column).

- [ ] Context view like the Claude Code one: colored square, name, tokens, percent per
      section, a segmented bar, free space, autocompact buffer; click a section for its
      details (which skills, MCP tools, memory files, MCP instructions, with counts);
      needs core: per-item breakdown and use counts of skills/MCP tools per chat and total
- [ ] Pasted screenshots and attachments go to the project's attachments folder under
      ~/Documents/duskcode, images in their own category (core: keep_pastes target)
- [ ] Connectors: a detail window for each MCP server, skill, plugin and agent; MCP
      tools switched on/off and described there (not in Tools)
- [ ] Tools tab: built-in tools only, as cards with a description window and a switch
      (core: ToolInfo needs a description)
- [ ] Agents graph: progressive disclosure. Start with category cards (the root's
      children), enter one to see its own subgraph with a breadcrumb; links to agents
      elsewhere shown as chips ("also asks X"), not as crossing edges
- [ ] Settings: edit max agents at work and goal rounds
- [ ] Arbiter: its own right-column window showing agents, their parents, messages
      between them, approval requests and the arbiter's answers; the arbiter is an
      animated dot sphere that pulses like a hive on each request
- [ ] Settings > System usage: what the GUI process itself uses (CPU, memory)
- [x] Version 0.71.0 locally, no tag
- [ ] Settings > Updates: commits ahead, the latest changelog, an update button
- [ ] Dark themes a little lighter (they are uncomfortably dark)
- [ ] Right click on a chat: delete (from the database and ~/Documents, confirmed,
      cannot be undone), rename, pin to the top
- [ ] Drag a chat to split the center into two chats side by side, each with its own
      right column
- [ ] Per-chat access mode (safety) under the composer; settings hold the default
      (core: per-thread approval policy)
- [ ] A click outside any open menu or window closes it
- [ ] Working indicator: an animated geometric dot figure, not a spinner circle
- [ ] Right column shows only modules that have something; Git renamed Changes;
      terminal button removed
- [ ] Chat scrolls up freely while the agent works
- [ ] display_image shows the real picture, not TUI pixels
- [ ] Agent actions folded: only the latest shows, expand to see the rest, expand one
      for its details
- [ ] Project environment: key/value variables for that project only (core: stored per
      project, applied to commands; values are secrets, never logged)
- [ ] Activity counts like the TUI (tools run, files changed, agents asked...)
- [ ] Context meter under the composer: a ring in four equal parts that changes color
      as it fills
- [ ] Chat column a little narrower; a + on the left of the composer opens a menu:
      files first, then folders
- [ ] The chat mode shown under the composer, Tab switches it
- [ ] Message navigator: a short vertical accordion next to the chat; on hover it grows
      into a catalog of ticks, each previews a sent message with its number, a click
      scrolls to it
- [ ] Send button: round and modern, not a square

## Later (v2)

- Internal browser panel, screenshots the model took, windows the model watches.
- Cron jobs: not in the core yet (only background jobs). Needs a core scheduler API first,
  then a right-panel section. Ask the user what a cron job may run (security: only the
  agent's own scripts, approvals apply).
- Plugin secret entry dialog, MCP server text editor, skill/agent editors, /migrate, updates.
- Image attachments in the composer and image cards in the transcript.

## Threat notes

- Assets: the user's files and git repos, provider keys, plugin secrets.
- The GUI never runs a shell; the terminal button passes the folder as one argument.
- Files and git only inside saved projects (core-side check, deny by default).
- Secrets go only through `Secret` and are never shown or logged.
- Model output is rendered as markdown text only: no HTML, no remote images in v1.
