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
sidebar/connectors.rs MCP servers, skills, plugins, agents with on/off switches
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
- [ ] check every panel by hand in the running app (the user reports bugs)
- [ ] model menu: close on a click outside (`on_mouse_down_out`), keys 1-6 pick a model
- [ ] Linux build in OrbStack (see memory `linux-checks`): needs Vulkan/X11/Wayland libs

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
