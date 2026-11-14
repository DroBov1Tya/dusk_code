# Structure audit

## Status, 2026-11-14

The crates now live under `projects/{protocol,core,server,clients/*,plugins/*,evals}`;
the paths below use the old names (gui = clients/desktop, tui = clients/terminal, web =
server, web-ui = clients/browser, external/telegram = plugins/telegram).

Done as moves and splits, with no behavior change: 2, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
17, 18, 19, 20 (`scripts/structure-map.sh` regenerates the map), 21, 22, 24, 25.

Then moved out of the clients: 3 (`core::app::git_summary`), 4 (`protocol::project`
parses schedules), 5 (`core::app::Uninstall`), 6 (`protocol::client::wav`), 23 (`now_ms` in
`protocol::system`). Of 1, what the desktop and the browser fold identically is in
`protocol/src/thread/transcript/`; one fold for all three clients needs decisions on what
each shows (the terminal keeps a different model) and is left. New candidates seen on the way:
`core/src/runtime/agent/agent/` (a module inside one of the same name), the desktop's
`pages/schedules/editor.rs` (602), `pages/settings/behavior.rs` (581),
`columns/terminal/mod.rs` (508), and the terminal's `app/panels/agents.rs` (447, three
reasons). On Linux, clippy 1.95 warns of an unread `Confinement::hidden` and of two
hand-written range checks; 1.99 does not.

## Findings as read on 2026-11-12

What does not fit `docs/structure.md` yet. Read on 2026-11-12 from the working tree of
`gui-polish-providers` (uncommitted work included). Nothing was moved. Line counts include
tests in the file. Rule numbers are those of `docs/structure.md`.

Clean already: `protocol` (flat re-exports, no file past 170 lines, no deep paths used from
outside), `core` root (three files), `tui` root (one file), cross-crate imports (no caller
names a file path behind a protocol front door).

## High: wrong domain or the same logic twice

| # | Where | What is wrong | Target |
|---|---|---|---|
| 1 | `gui/src/pages/chat/transcript.rs` (704), `web-ui/src/chat/data/mod.rs` (412), `tui/src/app/thread/transcript.rs` (515) | Rule 6. The same fold of history and `ThreadEvent`s into what a chat shows is written three times; gui and web-ui share type and function names (`Item`, `Tool`, `ToolState`, `Agent`, `from_history`, `add_message`, `tool_mut`, `agent_name`, `clip`, `summary`, `SHOWN_CHARS = 4000`). Which tool is shown, how steps fold and how agents nest are decisions, and they can drift. | One plain-data transcript in `protocol/src/thread/transcript/` (`item.rs`, `agent.rs`, `fold.rs`, tests from `web-ui/src/chat/data/tests.rs`); clients keep only layout state (heights, folds, image spots). |
| 2 | `core/src/util/` (`atomic`, `fs`, `notify`, `usage`) | Rule 3. `lib.rs` itself calls it "helpers with no domain". Each has one: `notify` and `usage::process` talk to the machine; `atomic::write_private`, `fs::read_regular`, `open_no_follow` are the private-file boundary (22 call sites); `usage::disk_bytes` measures the records. | `platform/notify.rs`, `platform/usage.rs`; `init/files.rs` (or a `files` module beside `init::create_private_dir`) for atomic and no-follow reads; `fs::printable` to its one caller; delete `util`. |
| 3 | `tui/src/host/git.rs` | Rule 6. The terminal runs `git` itself for the footer line while `core/src/project/git.rs` already reads branch, changes and ahead/behind as `protocol::project::GitInfo`. Two parsers, and this one runs outside the saved-project boundary. | Ask the core (`Request` under `client/request/project`), draw `GitInfo`; delete `host/git.rs`. |
| 4 | `tui/src/app/schedule.rs`, `gui/src/pages/schedules/editor.rs`, `web-ui/src/pages/schedules.rs` (`moment`) | Rule 6. Each client turns user input into a `Repeat` and a first run time (`daily 09:00`, `once 14:30` today or tomorrow) with its own date arithmetic. | `protocol/src/project/schedule.rs` gains the parse and `Clock`-to-moment helpers (it already holds `Repeat`, `Clock`, `interval_name`); clients pass text or fields. |
| 5 | `tui/src/cli/remove.rs` (192) | Rule 6. Uninstall decides what to delete (binary, links, desktop leftovers) inside a client; the GUI reaches it by spawning `duskcode remove --yes`. The guard is already the core's (`app::RemovalGuard`, `backup`). | `core/src/backup/uninstall.rs` behind `app::remove(paths, options)`; `cli/remove.rs` keeps the question and the printing. |
| 6 | `gui/src/pages/chat/dictation/pieces.rs` (`wav`), `web-ui/src/voice.rs` (`wav`) | Rule 6. Two WAV encoders in clients to feed one core transcriber. | `core/src/voice/` accepts samples and rate (request in `protocol`), encodes once. |
| 7 | `external/telegram/src/app/bot/{handlers,utils,workers}` | Rule 3: layer names (`handlers/buttons`, `handlers/callbacks`, `handlers/commands`, `utils/text`), 26 files for 1.5k lines, seven `mod.rs` with no doc. One feature (`question`, `approval`, `pick`) is spread over three layer folders. | `bot/question.rs`, `bot/approval.rs`, `bot/pick.rs`, `bot/pairing.rs`, `bot/owner.rs`, `bot/text.rs` (markdown to Telegram HTML), `outbox.rs`; each front door documented. |

## Medium: a file with more than one reason to change

| # | Where | Reasons | Split |
|---|---|---|---|
| 8 | `gui/src/pages/chat/messages.rs` (1104) | Virtual list and row sync; turn activity and agent report; step and command cards; pictures; turn cost. | `pages/chat/messages/` with `mod.rs` (list, `Row`), `activity.rs`, `step.rs`, `command.rs`, `picture.rs`, `cost.rs`. |
| 9 | `tui/src/session/actions.rs` (716) | One `apply` of 630 lines matching every `Action`, plus browser opening and editor helpers. It changes with every feature. | `session/actions/` by area, as `core/src/runtime/api` is: `thread.rs`, `settings.rs`, `panels.rs`, `files.rs` (editor), `browser.rs`; `mod.rs` keeps the dispatch. |
| 10 | `gui/src/workspace.rs` (708) | The root view's state (150 fields), the picker and column enums, core event handling, rendering. | `workspace/` keeping the type's path: `mod.rs` (struct, enums), `events.rs`, `render.rs`. |
| 11 | `gui/src/pages/chat/composer.rs` (745), `model_menu.rs` (557) | Composer: input, attachments, side marks, send. Model menu: drawing and keyboard navigation (`menu_keys`, `page_keys`). | `chat/composer/` (`mod.rs`, `attach.rs`, `marks.rs`); `chat/model_menu/` (`mod.rs`, `keys.rs`, `efforts.rs`). |
| 12 | `core/src/message/sources.rs` (669) | Storing sources of a thread; finding picture paths in text; decoding and shrinking pictures; building the block the model reads. | `message/sources/` with `mod.rs` (`ThreadSources`), `picture.rs`, `paths.rs`, `block.rs`, `tests.rs`. |
| 13 | `core/src/plugins/platform.rs` (601) | Wire format (`ToPlatform`, `FromPlatform`), keeping the process alive, the session of one platform, opening its thread. | `plugins/platform/` with `mod.rs`, `wire.rs`, `session.rs`, `thread.rs`. |
| 14 | `core/src/runtime/thread_runner/mod.rs` (393), `core/src/client/mod.rs` (290) | Front doors that hold the implementation and have no `//!` doc (rule 1). | Doc at the top; `ThreadRunner` impl to `thread_runner/runner.rs`, `Hub` and `Client` to `client/hub.rs`, `client/client.rs`; `mod.rs` declares and re-exports. |
| 15 | `core/src/tools/computer/screen.rs` (651), `overlay.rs` (590) | 16 flat files mixing tools (`click`, `scroll`, `type_text`), the screen backends (`macos`, `linux`, `accessibility`, `native_window`) and the overlay. `shared.rs` is a grab bag name. | `computer/backend/{macos,linux,accessibility,native_window}.rs`, `computer/overlay/`, tools stay flat; `shared.rs` becomes `offer.rs` (the macro) and `blocking` moves to its users' front door. |
| 16 | `core/src/runtime/` root: `thread_runner/`, `thread_lock.rs`, `thread/`, `launcher.rs` (550), `runtime.rs` | One concept under four names; `runtime::runtime` needs a clippy allow. `launcher.rs` both builds threads and carries messages between agents. | `runtime/thread/{runner/,lock.rs,start.rs,context.rs,...}`; agent mail from `launcher.rs` to `runtime/agent/mailboxes.rs`; `runtime.rs` body into `runtime/mod.rs` or `api/mod.rs`. |
| 17 | `gui/src/screenshot.rs` (429), `gui/src/keys.rs` (182) | Rule 4: features at the crate root. `keys.rs` binds shortcuts and also draws the shortcuts page. | `gui/src/dev/screenshot.rs` (feature-gated module); bindings to `gui/src/platform/keys.rs` or `ui/keys.rs`, the page to `pages/settings/shortcuts.rs`. |
| 18 | `web-ui/src/` root (11 files) | Rule 4. `sidebar.rs`, `voice.rs`, `markdown.rs`, `session.rs`, `theme.rs`, `ui.rs` are features, not entry points. `ui.rs` mixes widgets with number and time formatting. | `columns/sidebar.rs`, `chat/voice.rs`, `chat/markdown.rs`, `auth/session.rs`, `ui/{mod,theme,format}.rs`; root keeps `main.rs`, `app.rs`, `state.rs`, `link.rs`. |
| 19 | `core/src/state/{projects,project_env,schedules,thread_files}.rs` | Flat files beside `state/thread/` and `state/agents/`; the project records have three files and no door. | `state/project/{mod,env,schedules}.rs`; `thread_files.rs` into `state/thread/files.rs`. |

## Low: names and docs

| # | Where | What is wrong | Target |
|---|---|---|---|
| 20 | `docs/structure.md` map | Out of date. Missing directories: core `providers/google` (5), `runtime/goal/verification` (7), `state/thread/{conversation,goal,work}`; gui `columns/topbar` (3), `pages/chat/{audio,dictation}`, `pages/connectors/voice`, `pages/settings/providers` (6); tui `app/panels/settings`, `ui/thread/{input,transcript}`, `ui/windows/{panels,settings}`. Counts off: protocol `client` 16, `config` 12, `project` 6, `tools` 6; core `client` 15, `project` 7, `providers` 26, `secrets` 5, `voice` 15; gui `columns` 22, `pages` 58, `chat` 27, `connectors` 11, `schedules` 3, `settings` 13, `ui` 7; tui `app` 31, `cli` 7, `session` 6, `ui` 43; web-ui `chat` 16, `pages` 24. | Regenerate the map in the commit that changes the tree; consider a script so the numbers are never typed. |
| 21 | `core/src/runtime/events.rs`, `config/effort.rs`, `config/context.rs` | Three-line files that only re-export a protocol type. | A `pub use` in the module's `mod.rs`. |
| 22 | Front doors without `//!`: `gui/src/pages/connectors/voice/mod.rs` (255), `pages/chat/dictation/mod.rs` (192), `pages/chat/audio/mod.rs`, `tui/src/app/panels/settings/mod.rs`, `core/src/runtime/goal/verification/mod.rs` | Rule 1. | One paragraph each; move the drawing out of the two large gui `mod.rs` files. |
| 23 | `now_ms` (gui `chat/timing.rs`, tui `app/state.rs`, web-ui `chat/data`, core `client/serve/schedules.rs`), `clip` (five copies) | Small helpers copied per crate; `gui` reaches `crate::pages::chat::timing::now_ms` from other pages. | Go away with finding 1; what remains goes to `ui/` of each client. |
| 24 | `gui/src/pages/{file.rs,review.rs}` (412), `pages/artifacts/` (one file), `tui/src/ui/sidebar.rs` (623) | Pages as flat files beside page directories; a directory of one file; a column at the root of `ui`. | `pages/review/` (list, diff), `pages/artifacts.rs` or more files, `tui/src/ui/sidebar/` (projects, threads, footer). |
| 25 | `web/src/{server,bridge,guard}.rs`, `external/telegram/src/app.rs` beside `app/` | Root files that are modules (documented for `web`, three files, so acceptable); `telegram` mixes `app.rs` and `app/` where the rest of the workspace uses `mod.rs`. | `web`: leave. `telegram`: `app/mod.rs`. |

## Order to do them

1. Finding 20 (map) with any move below, in the same commit.
2. Mechanical moves with no behavior change: 2, 19, 21, 17, 18, 16, 15, 7.
3. Splits: 8, 9, 10, 11, 12, 13, 14.
4. Logic moved into the core or protocol (not mechanical, each its own feature branch with
   tests): 1, 3, 4, 5, 6.
