# Changelog

What changed in each release. A release's section here is also its notes on GitHub.
Nightly versions are pre-releases tagged on main; full releases are tagged on `release/X.Y`
branches after end-to-end testing. Entries of unreleased changes live in `changes/`.

## Unreleased

## 0.75.0 - 2026-11-18

### Added

- The Workshop reads store format 2 (kind, category, tags, cover, stars, dates) and
  mirrors added under `workshop.sources` in `config.yml`, each pinned to its own key;
  listings carry a tier (Official, Verified, Community, Local), sort in the core, offer
  semver updates, and "update all" skips updates that ask for more.
- The terminal client keeps threads in tabs: Alt+T opens one, Alt+W closes the shown one,
  Alt+Left/Right and Alt+1..9 move between them, and `/tab` does the same. Once there are
  two, the tabs stand in the header where the title was; a tab in the background keeps
  taking its thread's events and marks work, a question or an end not yet seen. Closing a
  tab stops nothing: a thread still at work is closed once its work ends, and its tab comes
  back if the agent asks something meanwhile.
- A running command in the terminal transcript offers a background button beside hide and
  copy; it sends the command to the background as Ctrl+B does, and the agent goes on.

- Two helpers so every client decides the same: `Checks::summary()` words the head of a Checks block ("3 passed · 1 failed"), and `Question::light()` says whether an `ask_user` question is short enough to be drawn as reply buttons.

- The web client keeps the blocks the agent shows with the `show` tool (a flow, a list of checks, reply choices) as their own items of the chat, live and after a reload, in place of the tool step of the call. Until each kind has its own view a block is drawn as its plain text in a frame marked "From the agent"; a call that was not shown stays a tool step. `show` is still off by default, so nothing changes until it is switched on.

- The web client now draws what the agent shows: a flow as stacked layers with cards and arrows, a list of checks with a glyph and word for each state and a summary, and reply choices as pills under the answer. A short `ask_user` question is drawn as the same pills instead of the card, with "Other..." opening the card. Pills answer only while they are the last thing in the chat. A `{{tone: label}}` in an answer is a coloured label in one of six tones. The working row keeps its place while the turn runs: elapsed time in a box of fixed width, and the words change for a wait on the provider or the user and for compaction. All of it is text from the model drawn as text; `show` is still off by default.

- A change ledger: the files each command changes in its project are found by walking the project before and after it, with or without git, and recorded with their thread, turn and call, sizes, a content hash and, when the text before was read earlier in this run, a capped unified diff (never for `.env*` files or anything under `.ssh`). Recording is on by default (`limits.record_changes`) and also feeds the chat's list of changed files; clients read the ledger with `WorkRequest::FileChanges` (protocol 16). The agent's read-only `changes` tool asks it by path, time and thread, and stays off until `tools:` switches it on.

- Community files: a Code of Conduct, issue forms for bugs and feature requests, a contributor path for a first change and for writing a skill, and a README note on the maintainers' working notes.

- Compaction that keeps the recent tail, behind `limits.compact_tail` (off by default): the summary covers only the older part, and the newest messages, at most `limits.compact_tail_tokens` (16,000, from 2,000 to 64,000) and never more than a fifth of the input limit, stay word for word; a cut never parts a call from its result, and when none keeps a tail everything is summarized as before. `limits.compact_early` (off by default) also compacts between turns once the context passes 60% of the input limit. With `limits.stub_results` on, a compaction puts the kept tail's old results away at once and its summary ends with an index of up to 30 stored results it covers. The summary prompt asks for a "Decisions" section only with `compact_tail` on and a "Stored results worth reading" section only with `stub_results` on; with both off it is unchanged.

- Computer use: `computer_actions` runs up to 20 clicks, typing, keys, scrolls and
  screenshots in one call, checked and asked for together before the first runs, with
  every click read in the picture taken before the batch. Every client and chat platform
  lists each step of the request; tool switches, plugin hooks and the arbiter judge each
  step as if it were called alone. `press_keys` now refuses line breaks and control
  characters in the keys and shows them by their normal names. A chat platform
  gets a request whole or cannot allow it: one too long for its message can only be
  denied there and is answered in the app.

- An optional solid fill for the desktop's destructive confirmations: a palette that sets `danger_button` (all built-in ones do) draws Remove, Delete and Remove all as a clear red button with a readable label instead of the kit's pale tint; a palette without it looks as before.

- The desktop's transcript keeps a block the model showed with `show` (a flow, checks or reply buttons) as its own item and row, in place of that call's step, live and in a reopened chat alike; until each kind is drawn it shows as its plain text in a card marked "from the agent". A call that was not shown stays a step, and nothing changes while `show` is off. The working row moved into its own file.

- The desktop draws what the model shows with `show`: a Flow as stacked layers with their cards in a grid, a Checks list with state glyphs and its summary, and reply buttons under the answer. A short `ask_user` question is answered with the same buttons instead of a card; they are live only after the turn ended and while nothing came after them. The working row has fixed boxes for the elapsed time and the step, shows the provider wait and the wait for your decision with the protocol's words, and its figure stands still for a decision or with reduced motion.

- Desktop: a plan strip above the composer, one line with the plan's counts from the core, its bar and the task in work, unfolding (Cmd+P, Ctrl+Shift+P) to numbered rows with a state glyph and word, six in sight in a virtual list; and a views tab on the composer's edge (Cmd+E, Ctrl+E) whose menu opens the Overview. The board follows `TaskChanged`.

- Episodes are written down, behind `limits.episodes` (off by default), for a chat whose folder is a saved project: a `fixed` one when the same error came back twice or more in a turn and then a foreground `run_command` of the same program and first argument exited 0 (the error that failed last, with the attempts, the passing command and up to four files the change ledger recorded in between; three a turn at most), and a `verified` one when a goal's verification passes (its objective and its command checks). The failure ladder's count now also runs for `limits.episodes`; its notes still need `limits.failure_ladder`. With `limits.episode_lesson` also on, the result of the passing call asks the model to end its answer with a `Lesson:` line, which is cleaned and stored on the turn's episodes. A new `EpisodeRecorded` event says an episode was written, and the eval runner counts it as `episodes`; the protocol is at version 19.

- An episode store in the state database (migration 0035): a project's episodes, each what ended a problem in it, with a trigram search index, counted again instead of copied when the same one recurs, at most 500 a project (the least seen, oldest go first), kept when their chat is deleted and removed with their project. Searches take only the words of a query, each as a quoted phrase, so query text is never read as search syntax. Nothing writes or reads episodes yet.

- The eval runner records a token ledger per run (cached, uncached and cache-written prompt tokens, completion and reasoning tokens), provider retries and re-runs of identical tool calls, and prints tokens per solved task next to the pass rate. A timed-out run now keeps what it spent and is left out of the per-run means. `--set NAME=VALUE` changes a key of the run's config to compare a mechanism on and off, refuses unknown keys, and is recorded in the report so `--compare` shows what differed.

- A failure ladder for a model stuck on one error, behind `limits.failure_ladder` (off by default): when the same error comes back within a turn through calls that differ, the third time its result tells the model to stop varying the call, the fifth asks for a different plan, and the eighth ends the turn as failed with the error and the attempts, without running the rest of that step's calls; a subagent reports that failure to its asker, and a goal pauses. Two errors are the same when the tool, the program a command runs and the last error lines match once numbers, paths, durations, hex ids and quoted text are made alike; digits inside a name such as `test_1` stay, so working through numbered tests is not a loop. The thresholds are `limits.failure_note_at`, `failure_replan_at` and `failure_stop_at` (3, 5, 8). A new `RepeatNotice` event says when the core stepped in, for the existing note on an identical call too, and the eval runner counts it as `repeat_notices`, with turns it stopped as `repeat_stops`; the protocol is at version 13.

- Git from the app: the core stages, commits, branches, merges with a conflict list, adds and removes worktrees, fetches, pulls fast-forward only and pushes without force, and reads and opens pull requests through `gh`; the desktop Changes page stages, commits, makes branches, pulls and pushes (protocol 27).

- The indexer of the project graph: with `limits.project_graph` on (off by default), one background worker keeps the symbols and refs of each saved project current. It refreshes a project when a thread starts in its folder, indexes the files the change ledger recorded as they change, and refreshes at the end of a turn the ledger could not record. A file whose size and change time are the stored ones is not opened, links are never followed, `.env*` files and `.ssh` are never read, and a project past 50,000 files, 20,000 source files or 1,500,000 refs has no graph. Nothing reads the index yet.

- The parser of the project graph: one Rust file in, the names it defines (functions, methods, types, traits, constants, modules, tests, each with its parent and its lines) and the names it refers to (uses with the first line and a count, imports with their path as written) out. Nothing calls it yet and no switch turns it on; the store and the indexer come next. It reads source as untrusted input: files over 512 KiB, lines over 5,000 bytes, more than 2,000 symbols and parses over 250 ms are skipped with the reason, names are identifiers of at most 128 characters, and the text of strings and comments is never kept. Adds `tree-sitter` 0.27.2 and `tree-sitter-rust` 0.24.2, pinned exactly and linked statically.

- The store of the project graph, behind switches that stay off: migration 0034 adds `graph_files`, `graph_symbols` and `graph_refs` (the structure of a saved project's source, never its text; removed with the project), and the core can replace one file's rows in a transaction, remove files, read stamps and counts, and answer `where`, `uses`, `touches` (with the files the change ledger saw changed in the same turns) and `tests` from it. Nothing builds the index yet. `limits.project_graph`, `limits.episodes` and `limits.episode_lesson` are new switches, all off, read by nothing yet; the tool names `project_graph` and `recall` stay off until `tools:` switches them on.

- The `project_graph` tool: the agent asks where a name is defined, which files use it (rows matched by name alone start with `?`), which files depend on a file, with its tests and the files changed with it, and which tests cover a file or a symbol, answered from the project's index in at most 16 rows and 2 KiB. A file changed on disk since it was indexed is marked and indexed again. It stays off until `tools: { project_graph: true }` and is offered only once the index of the thread's project is ready in this run (`limits.project_graph`).

- Pinned messages in the core: `ThreadRequest::Pin` pins or unpins one of a chat's messages (at most eight, the user's and the agent's words), `ThreadRequest::Pins` reads them, and `ThreadEvent::Pins` tells every client of the chat after a change or an /undo. A pin carries a plain one-line excerpt the core cuts to 120 characters and whether a compaction summed its message up; it goes with its message and its chat, and nothing of it reaches the model. Protocol 26. The desktop shows a 36 px bar of the pins at the top of the chat (a click or Cmd+Shift+P goes to the message, the chevron lists them all with an unpin button) and a pin button among a message's tools; the terminal names the first pin in its header and takes `/pins`, `/pin` (the last answer) and `/unpin N`.

- The plan's store (migration 0036): a task has a status (open, claimed, running, blocked, review, done, failed), the tasks it depends on, the files only it may change, an owner thread and a lease; `done` stays for older clients and always equals `status = 'done'`. A plan is the tasks of a chat's root thread, shared by the agent threads under it and reached from no other chat. The store adds many tasks in one call with dependencies between them, refuses a cycle, answers what is ready, gives a task out atomically, and makes every other move of the state machine as one checked transaction. Nothing calls it yet: the board and its tool work as before.

- The lead's plan form of `tasks`, behind `limits.plan_graph` and only for a chat in ORCHESTRATOR mode: `add` writes the whole plan in one call (keys, `after`, role, files, acceptance, gate or confirm, brief, phase), `edit`, `show`, `review`, `fail`, `reopen`, and `run`/`stop` through a seam the team runtime fills. Files are resolved inside the project, roles are checked against the agents catalog, gates by the goal's rules; a call run again after a restart changes nothing more. Anywhere else the tool is byte for byte the board, which no longer touches a task the team has given out.

- A turn whose provider cannot serve it now (rate limited, out of quota, out of reach, overloaded) waits and goes on by itself instead of failing: the wait is stored with the turn, outlasts a restart, ends at the user's stop, ends early at the user's message, and gives up after a day with the same `TurnFailed` as before (`limits.wait_for_provider`, on by default; the eval runner turns it off). A context too large for the model is compacted and the step tried again, twice at most, and with `limits.shrink_context` (off by default) a summary whose own request is too large covers the older half. A turn a restart resumes now tells a bot it is at work, an agent stopped by a restart wakes the agent that asked it, the answer to a restart's question reaches the thread with no model chosen and is never taken from an older tap, and the texts about a cut-off call say it may still be running.

- The `recall` tool: the agent asks what ended a problem before in this project and gets up to three notes from earlier turns (an error that kept coming back with the call that passed, or a check that passed), at most 2 KiB, only from the project of the thread's folder. It stays off until `tools: { recall: true }` and is offered only while the project has an episode; nothing records episodes yet.

- A turn cut off by a restart now goes on where it stopped instead of ending as interrupted: a call that may simply run again is run, and one that may already have taken effect is never repeated without asking (Continue, Check first or Stop); an agent in that case stops and its chat is told. A turn left more than `limits.resume_within_seconds` (an hour by default) asks before it spends tokens, and an interrupted goal review runs again while attempts remain. Calls skipped because the same error kept coming back now say so instead of blaming the user.

- Shaped command output, behind `limits.shape_output` (off by default): the output of `cargo` (build, check, clippy, test), `pytest`, `npm`/`pnpm`/`yarn`, `tsc` and git transfers is sent to the model without build progress, passing tests, install notices and transfer progress, with identical warnings and lines folded into one with a count, and ends with a line saying how many lines were left out and that `read_history {"result": N}` returns the whole output, which stays stored. Errors, failures, warnings and `file:line` lines always stay; piped or chained commands, programs without rules of their own (their output may be a file's content), diffs, file dumps and lines that cannot be followed are sent unchanged, repeated lines included. The shaped text is decided once when the result is stored and kept in a new table, so requests stay the same bytes. Clients get `ThreadEvent::ResultShaped` (protocol 18) and ignore it; evals count shaped results and the tokens they save.

- Workshop items can show up to 8 screenshots with captions in their detail window: format 2 lists name them like covers (`.png` or `.webp`, at most 1 MiB, checked by size, SHA-256 and first bytes), listings carry their count and captions, and `StoreRequest::Screenshot` fetches one only as the last signed list named it. The cover response is now `Response::Media`, shared by covers and screenshots.

- The `show` tool lets the model put a structured block in the chat instead of drawing it in text: a flow of layers joined by arrows, a list of checks with their states, or two to four reply buttons under an answer. In its own text the model can write a coloured label as `{{success: Passed}}`, from a fixed set of tones each client maps to its theme. Blocks are stored with the call that made them and read back as plain text by `read_history`; The tool is off until the clients draw blocks; `tools: { show: true }` in the config switches it on (see `docs/blocks.md`).

- Eval reports count stub moves, the results they put away and the tokens they saved, `read_history` retrievals of one stored result, and finished compactions; the means line and `--compare` show them, and older reports still load.

- With `limits.stub_results` on, old results of `run_command`, `read_file`, `search_files` and `read_history` are put away before a step when it pays for the provider cache it breaks: each is sent as a one-line stub that names the `read_history {"result": N}` call that returns it, `run_command`'s description says to read one back instead of running again, and the thread reports each move as `ThreadEvent::ResultsStubbed` (protocol 17). Moves never touch a message at or before the newest stub, and Anthropic providers move only once the cache is cold. It stays off by default.

- The `read_history` tool can return one stored tool result by its message number, paged by lines or searched with context; it stays off until the stub store brings its switch. The read-only `jobs` and `changes` calls run together with other reads, and files read together with `paths` feed the change ledger like a single read.

- Stubs for old tool results: a stored stub can be sent in place of a tool result's text, with the context size counted to match (`limits.stub_results`, `stub_keep_steps`, `stub_min_tokens`). Nothing changes until `stub_results` is switched on, and nothing writes stubs yet; the full text always stays stored.

- Eval tasks for teams, from the design of the plan graph: `team-fan-out` (four independent modules), `team-chain` (schema, API and command line, each needing the one before), `team-chain-kill` (the chain with a kill) and `team-small` (three short tasks at once, to see what a plan costs when none is needed). Each part has its own files and check command, and the tasks run for one agent as well. A task's `run.yml` may set `kill_after`: the runner kills the core at a tool event picked by `--seed` and starts it again on the same folders, to test recovery; the report records where.

- The protocol of the plan graph and teams (protocol 21): a `Task` now carries its `status` (`TaskStatus`: open, claimed, running, blocked, review, done, failed), dependencies, role, phase, acceptance, brief, gate (a `GoalCheck`), files, owner, attempts, reworks, what it waits on, note, model and the steps, tokens and changes of its turns; each new field has a default, a task without a status takes it from `done`, and one whose `done` and status disagree is refused. New requests `WorkRequest::PlanTasks`, `Team`, `ConfirmTask`, `StopTeam` and `SetTeamBudget` (a `TeamBudget` refused past the largest ceilings), answers `Response::PlanPage` and `Response::Team`, and events `ThreadEvent::TaskChanged` and `ThreadEvent::Team`. Scopes classify the requests (reads as thread reads, confirm and stop as thread writes, the budget with the approval mode); the core does not serve them yet and answers with an error.

- The team's runtime in the core (`runtime/team/`), behind `limits.plan_graph`, which stays off: with it on, in ORCHESTRATOR mode, the lead's `tasks` `run` has the core carry out the plan with no model call of its own. It gives each ready task to a member, a thread of its own under the chat with the task as its first message, the chat's approval mode and no tool to start agents, change the plan or write memory; keeps a 120 s lease on the task while the member works and gives it out again when the lease runs out, three times at most; takes the member's answer as the hand-in; and ends the team when every task is done, a budget of steps, tokens or time is spent, it is stopped, or nothing can start and the lead changes nothing. The lead is written to in one line of 400 bytes at most, only when a task waits for it, failed or is blocked, and at the end. Command gates are not run yet: such a task waits for the lead's review.

- The plan and team requests are served: `PlanTasks` pages a chat's plan, `Team` reads its team, and `ConfirmTask`, `StopTeam` and `SetTeamBudget` change it as the user, each announcing `TaskChanged` or `Team` to the clients that may read the chat. A task now carries all its fields on the board too, with its steps, tokens and changed files counted from its turns.

- The team's store (migration 0037): one team a chat, kept on its root thread, with a status, the reason it ended and a budget of members at once, steps, tokens and minutes. A run starts within the user's ceilings and, after an end, starts again in the same row; the lead may only lower a running team's budget, the user may raise it up to the ceilings. What a run used is counted from the model calls of the chat's whole thread tree since it started, never stored, and one check tells the dispatcher whether another task or step may go ahead or which budget is spent. Ending a team gives its tasks at work back to open. A team is reached only through a thread of its own chat. Nothing calls it yet.

- Settings for teams of agents, all off or at their defaults and read by nothing yet: the `limits` switches `plan_graph`, `task_packets`, `member_worktrees`, `peer_messages` and `procedures`; the ceilings `team_at_once` (1 to 8, at most `max_agents_at_work` in effect), `team_max_steps` (10 to 5,000), `team_max_tokens` (100,000 to 100,000,000) and `team_max_minutes` (5 to 1,440); and a `team` section with `model` and `models` per role, as `provider/model`.

- The terminal shows a block the agent made with `show` (a flow, checks or reply choices) as its own entry in place of the tool step, live and in a reopened chat, as its plain text under a head that says it is from the agent; the boxed drawing comes later. Labels `{{tone: label}}` in the agent's text are drawn in the tone's colour on the raised surface (a new `WARNING` yellow for warnings), stay whole when a line wraps, show as `[label]` with `NO_COLOR`, and copying a message copies their words without the braces.

- The terminal draws the blocks the agent shows: a flow as a box a layer with its title in the border, cards in columns and an arrow between layers (the emphasised layer in the accent; under 60 columns it stays plain text), and checks as one line a check with a glyph for its state, its counts at the right and the summary above. Both still say "from the agent". `show` is still off by default. The terminal also shows reply choices from the agent, and a short `ask_user` question, as one row above the message box (`reply · from the agent  [1] Yes  [2] No`) while they are the last thing in the thread; Alt+1 to Alt+4 answers with the label (while the row is shown they take the place of the tab shortcuts of the same number), and a short question also takes the plain number or Enter on the row under the cursor. A longer question keeps its card.

- The working row shows what the running turn has used, counted by the core: `Working · 2m 14s · 18.4k tokens · step 7` in the desktop and browser chat, ` · 18.4k tok · step 7` in the terminal footer. A new `TurnProgress` event carries the turn's steps, tokens and cost after each model call, and every `Turn` carries the same totals, so a chat opened mid-turn and a finished turn's fold show them too. Clients no longer count steps or diff the thread's spend, and they share one number format (`short_count`). Protocol version 22.

- A running turn can wait, for the user's decision or for a provider until a set time, and the wait is stored with the turn (migration 0030) so a restart finds it; clients get a `TurnWaiting` event and one shared line for it. Protocol version 15.

- The Workshop installs skills, skill packs and agent packs, not only plugins: one package for any system, unpacked under the plugin rules into `skills/` or `agents/`; updates and removal keep files you edited. Skill packs, the built-in skills as `duskcode` among them, can be switched off whole (`skill_packs:` in `config.yml`). Two built-in skills are new, `computer-use` and `token-thrift`; the other skills and most agents moved to Workshop packs, and copies earlier versions installed stay as your own.

  A Workshop pack must name the skills and agents it links to by path (`duskcode-agents/backend-engineer`); an ignored test (`DUSKCODE_WORKSHOP_DIST`) installs the built packs and checks every link resolves.

- The Workshop is a storefront in the desktop app, the terminal and the browser. A featured hero of official picks with a strip of their covers, then shelves of capsules: Official, Verified, Community, the core's New, Recently updated and Top rated picks (`StoreList.picks`), and the built-in and local packs, each with "See all". Capsules keep the art's 1200x630, show Installed or Update on the cover's corner and warn when another source offers the same name. One Filters button beside the search opens Sort, Category, Tier and Kind, and what is on shows as removable chips. A listing opens a window like a store page: a gallery of its cover and screenshots with captions, arrows, a strip and the Left and Right keys, its facts, tags, buttons, warnings, permissions, page and dates. A pack has a page with its skills and their switches. Sources that could not be read get a line each, installing names the source and tier, and "Update all" shows what was updated and what was left with why. The browser browses and switches but does not install; the terminal keeps its keys (Tab for the category, Ctrl+O for the order).

### Changed

- An answer to the agent's questions is one rounded card in the desktop, browser and
  terminal clients: a row for each question, the chosen options and your own words muted
  under it, "Declined" for a refusal. It is read back from the real `ask_user` calls, so
  text that only looks like an answer stays a plain message.
- A model call that stays silent can be given up on sooner or later: Advanced settings now
  hold the seconds of silence before a call is dropped and the tries of one call, in every
  client. The defaults are the old 120 seconds and 6 tries.
- Icons: the changes, the branch and Version control use git icons instead of the GitHub mark, and Connectors uses a blocks icon, in the desktop and browser clients.
- Phone panels are calmer: a toolbar's device menu is the device's name over its
  system, icon buttons are small with their names as tooltips, a centered row of icons
  is one small bar under the screen, and Shut down stands apart behind a rule and warns
  under the pointer. The phone grows to fill the panel, with a thin bezel, a light rim
  and the screen centered in it.

- The Workshop shows stars only where they are honest. A listing of the official list or of a verified mirror shows none: its count was the Workshop's own repository's or a vendor's whole repository's, not the item's, and the core drops it whatever the list says. A listing from a mirror you added shows its count only beside the GitHub repository its page names, as "122 GitHub stars of owner/name", on the listing's page, never on a card. The "Top rated" shelf and the "Most stars" sort are gone from every client, so a mirror cannot push its items up by writing a big number. `Listing.stars` became `Listing.repo_stars`, and the protocol is version 24.

- The Statistics module is now the Overview in every client: progress and members of the plan's team, tokens, context, changes, tool activity and an event log, each section absent when it has nothing to say. The new `WorkRequest::Overview` and `WorkRequest::Events` (protocol 23) answer from the core: tool runs from `tool_executions`, lines changed from the change ledger, and the log from the thread's turns and team, so no client counts them any more. The desktop draws it in the right column and as a page ("Open"), the browser as a tab that widens, the terminal in the rail (with the plan's line and members) and in `/overview`.

- A failed model call now says what happened, such as "could not reach api.example.com: connection refused", instead of "provider completion request failed". Provider failures carry a kind (unreachable, timed out, rate limited, overloaded, auth, quota, refused, too long, interrupted) that decides retries and reaches clients in the `Retrying` event with the planned wait and the attempt limit; the log gets the underlying cause without URLs, keys or request bodies. Retry jitter comes from the system's random source. Protocol version 12.

- `read_file` can read several files in one call: with `paths` (2 to 8 files, each from its start) every file comes back in a section headed by its path, a file that cannot be read gets its own error line, and the sections share the 64 KiB cap equally; it is behind `limits.read_many_files` (off by default), and with it off the tool is offered exactly as before. The read-only calls of one step (`read_file`, `search_files`, `read_history`, `load_skill`, and MCP tools that say `readOnlyHint`) now run together, at most 8 at once, with results stored in call order as before; `limits.parallel_reads: false` runs them one by one.

- Releases follow the rust-lang model: nightlies are tagged on `main`, beta pre-releases on `beta`, full releases on `stable`; `main` is promoted to `beta` and `beta` to `stable` by merge commit, and the release workflow and CI follow these branches instead of `release/X.Y`.

- Tools now say whether a call may be repeated after its result was lost; nothing uses it yet.

- When a provider fails, the chat shows one retry row that updates in place with a countdown to the next attempt, instead of a growing list of retries. The providers settings page shows whether each provider is answering or failing.

- A provider that cannot answer no longer fills the chat with a row per attempt and two error rows. A model call being tried again is one state that each `Retrying` replaces and the call's first answer ends (`transcript::retry_row`), and a failed turn ends with one `TurnFailed` event that carries the kind and the number of attempts; it is stored with the turn and shown again when the chat is opened. A host that cannot be reached, or does not answer in time, is tried for `limits.retry_unreachable_seconds` (300 by default, 10 to 3600) instead of `max_attempts`, so a short network drop costs no turn; the event's `max_attempts` is then empty. The status carries each configured provider's health (answering since, or failing with a kind since, and when it last answered), and changes only when that class changes. `ProviderUnavailable` is now `NoModel` and means only that no model is chosen. Protocol version 13.

- The `show` tool is on by default, since every client draws its blocks; `tools: { show: false }` switches it off. The desktop draws the labels in an answer (`{{success: Passed}}`) in their tone, as the browser and the terminal do. `scripts/clean-build.sh` removes build artifacts not used for a week.

- With `limits.stable_prefix` (off by default; the eval runner turns it on with `--set`), a thread keeps each tool's description as it first showed it until its next compaction, so a memory rewritten through `remember`, or a skill or an agent added mid-thread, no longer changes the start of every later request and the provider's cache of it holds. Tool schemas no longer carry the integer widths schemars names (`"format": "uint64"`) or `null` in an enum of a field that is a string, which every request paid for.

- The finished features are on by default: `failure_ladder`, `shrink_context`, `stable_prefix`, `read_many_files`, `stub_results`, `compact_tail`, `compact_early`, `shape_output`, `project_graph`, `episodes`, `episode_lesson`, and the `changes`, `project_graph` and `recall` tools. Each can be switched off in `config.yml` (`limits:` and `tools:`). The team features stay off.

- The protocol is version 20: the Workshop's `StoreRequest::Screenshot` and `Response::Media` changed its shape. A screenshot is in the same scope area as a cover, reading the store, so a browser may fetch it and a scope bound to chats may not.

- The Workshop panel in the terminal uses up to 100 columns instead of 70, and shows a listing's kind in brackets at the right of its name. Bidirectional controls in store text are removed with the other controls.

- Links to the Workshop use its new name, `duskcode-workshop`: the repository was renamed and its page moved to `https://drobov1tya.github.io/duskcode-workshop/`.

### Fixed

- Pictures in the desktop chat keep their proportions and are never enlarged, and all of them
  have lightly rounded corners.
- The interim remarks of a running turn no longer fold away after the next step when the
  desktop or web client opens a chat whose turn was already running; a chat is now in
  place before its thread opens, and a client that joins mid-turn is told the turn is
  still going.

- The browser's Workshop draws the desktop's card, placeholder, grid and hero. A card has one height whatever its text: the cover at 40:21, cropped to fill, with nothing drawn over it; the name and kind; two lines of text, the first taken by a warning (a name another source offers, or why it cannot run here); the tier mark and one slot that says the state or that installing happens in the app. A listing without a cover shows its kind's glyph, drawn in the page, with a shimmer while the cover comes and an "Offline" mark when it failed without a network; a failed cover is asked again on the next read. The hero, the shelves and the results share one grid of 1 to 4 columns from the page's measured width; a shelf shows one row and pages by a row with its arrows instead of scrolling sideways. The hero is 280 px high with dots and keeps the keyboard focus when it changes.

- The Workshop's listing window keeps the gallery's caption whole above its fold in a short window, a cover still coming shows the kind's quiet glyph and "Loading" instead of an empty box, and the remove confirmation starts with the keyboard on Cancel and its Remove button outlined in the danger colour.

- Desktop Workshop: a card's cover is rounded inside its border at the card's inner width, nothing is drawn over it, and every card of a row is one height with two lines of text and a footer of the tier mark and one slot (a button or a state). A listing without a cover shows a quiet placeholder drawn in code with its kind's glyph, an empty box while the cover comes, and "Offline" when it could not be fetched while the store could not be read; the pale letter tile is gone. The hero, shelves and results share one grid of one to four columns measured from the page's own width, and the hero is one card 280 px high with dots instead of a strip of thumbnails. A listing's window puts its cover in a header, shows only screenshots in its gallery on a 16:10 stage fitted whole, gives Remove the danger colour apart from the main action, writes dates as "11 Nov 2026" and shows its page's host and path; a link that names a user before its host is not shown. Skills, tools, MCP servers and installed plugins are rows of one card instead of fixed 360 px cards that left the right edge ragged. A listing at work shows its turning ring beside "Installing".

- The Overview opened as a page has a Back button and Escape returns to the page it was opened from, and its progress bar stays one bar (one segment a status, not a segment a task) with 200 tasks, in the desktop and the browser client. The `fake=overview` screenshot step shows a board of 200 tasks, and the `screenshot` build compiles again.

### Removed

- Design documents, plans, measurement logs and HTML mockups left the repository (they live with the maintainers); `docs/features.md` now says what the shipped features do and which switch turns each on.

### Security

- CI checks the supply chain on every pull request: cargo-deny (RustSec advisories, licenses, crate sources), osv-scanner over the lockfiles and gitleaks over the history; Dependabot proposes weekly cargo and action updates. CI also builds the web client for wasm and checks Linux arm64; releases are built with the same Rust 1.99.1 as CI and carry a CycloneDX SBOM of each program.

- Every client attached to the core now carries a scope, and the core checks each request against it before anything runs. The terminal and the desktop app have the full scope. A browser has the same reach as before, but the core enforces it now instead of the web bridge: no stopping the core, no terminal, no plugin parts, no Workshop installs, no switching a plugin on, no file writes and no config text of a saved MCP server. A scope can also be bound to chosen chats, which then are the only ones a client can read, drive, list or hear from; nothing uses that yet.

- Episodes are cleaned before they are stored: every text of one is made plain as a stub is (one line, no invisible or bidirectional characters, no brackets of its own, so it cannot pass for a harness note), loses every value of the project's variables (`<secret>`) and everything shaped like a secret (`<redacted>`: the value after words such as `token`, `password` or `Authorization`, keys with a known prefix, PEM blocks, the user part of a URL, long runs of base64 or hex), and is cut to its column. An episode is not written for a folder that is not a saved project's, or when one of its commands names an `.env*` file or anything under `.ssh`. Nothing records episodes yet.

- The project graph could store text from a comment as a name. While tree-sitter recovers from a syntax error the Rust grammar reads comments and strings as code, so a stray byte followed by a doc comment turned the words of the next comment into symbols; and a comment inside a `use` path was stored with the path even in a file with no error. The text of every string and comment is now replaced by spaces before the parser reads a file, by a lexer that follows the compiler's rules, so in a file the compiler can split into tokens none of it reaches the index. A file with unpaired quotes is still best effort: what follows the broken quote is dropped.

- Fixes from a security review of the project graph, episodes and shaped output. The graph now also caps a project at 400,000 symbols, and a refresh at 50 files too slow to parse or 30 s of reading and parsing; past either the project has no graph, as past the refs cap. An episode is cleaned from the whole failing command and error lines (up to 16 KiB) and cut only afterwards, so a key or a project variable cut in half by the failure ladder's 100- and 400-character listing no longer escapes redaction, and a `.env` named late in a long command is refused; the ladder's notes to the model are unchanged. Changing a saved project's folder now deletes the episodes and graph of the old folder instead of carrying them to the new one, and removing a project or an episode updates whether `recall` is offered. Shaped output no longer folds repeated lines for programs it has no rules for (their output may be a file's content), and a cargo warning counted by its rule is not counted again.

- The MCP client takes rmcp 3.10, which caps the body a hostile MCP server can send (RUSTSEC-2026-0462).

## 0.74.4 - 2026-11-16

### Fixed

- The core compiles on Linux again: the macOS overlay helper stays on macOS, and
  plugin executable lookup has an explicit path type on Linux.

## 0.74.3 - 2026-11-16

### Added

- On macOS, the iOS Simulator and vphone panels can install AXe through Homebrew or
  download the notarized vphone Launchpad. Both panels recheck their setup afterward.
- Phone panels keep the device picker and Refresh together and put Home, Lock and
  Shut down controls below the live screen.
- The Desktop composer shows ChatGPT plan usage for each reported window and links to
  usage settings.
- Answered questions show the question and selected options in the chat, and background
  job results have their own compact row. Unclear answers stay as text.

### Changed

- Desktop Settings moves model and voice setup under Agent → AI. Image and video
  generation tabs are placeholders and do not yet generate media.
- The Desktop navigation rail uses a house icon for Chats.

### Fixed

- The message box keeps what is typed and attached for each chat: another chat shows its
  own unsent text or an empty box, in the Desktop, the web page and the terminal.
- A Desktop chat keeps its scroll position as a long message grows, and follows the end
  again when the reader returns there.
- Desktop AI settings align the Voice cards and fit the category controls in narrow panes.
- Desktop keeps assistant text in order with collapsed activity and leaves retry
  messages visible during a turn.
- Chats make at most one automatic title request after their first turn. A title chosen
  by the user is kept.
- ChatGPT sign-in records cache writes when the provider reports them. OpenAI Responses
  requests also carry a stable prompt cache key for the thread.
- Invalid configuration YAML reports the location and a safe diagnostic without echoing
  arbitrary unknown keys or values.
- Desktop Settings → About me reports whether the profile and snippets are saved, and
  keeps an unsaved profile when the page is reopened. The web page confirms when the
  profile has been saved and reports save errors.

## 0.74.2 - 2026-11-15

### Added

- Plugins may run Xcode's tools (`exec: [xcrun]`) and connect to the Unix sockets of the
  folders they name (`sockets:`), both shown before a plugin is switched on.
- Pictures an MCP tool returns reach the model, checked like any image.
- A picture sent into a chat now shows as a thumbnail in your message; a click opens it
  large over the window, closed by Escape or a click outside. The bytes come from the
  core, by the source's name, never read from a path by the client.

### Fixed

- OpenAI-compatible servers now receive the output budget their model catalogue reports,
  so a server's smaller default does not cut a reasoning model off before its tool call.
- The model's stream is no longer cut by estimating the provider's tokens from byte counts.

- A file or picture sent into a chat is no longer re-sent in full on the next turn when
  a turn that read it ended by failing (such as a ChatGPT stream error), being
  interrupted, or calling tools. A source counts as delivered once any model call of the
  turn took it.
- A tool step's Arguments and Answer can now be selected and copied in part, not only in
  whole through the copy button.
- A used-up ChatGPT plan now says so, instead of "the provider is unavailable" after
  five retries; a chat too long for the model says to compact it.
- The log no longer warns on every ChatGPT answer that it came without a content type.
- A chat scrolled up to read stays where it is when the agent starts a new thought,
  instead of jumping higher.

- The weekly window of a ChatGPT plan now shows in the limits ring: it is read from the
  answer's stream as well, and kept per account rather than per renewed token.

### Changed

- The browser workspace now follows the desktop layout with a navigation rail, a chat list
  and an inspector. On smaller screens, navigation and the inspector open when needed.
- The model menu now shows only the model in use; every other model is chosen under More
  models, where the provider in use opens unfolded and its first six models keep their
  number-key shortcuts.
- The iOS Simulator panel and the agent's `ios_simulator` tool move from the app into the
  plugin ios-simulator, installed from the Workshop. A saved window layout that still
  names the simulator loads without it.

## 0.74.1 - 2026-11-14

### Added

- Plugins can add parts to the desktop window: an icon in the header, the rail or beside
  the message box, or a page under Settings, opening a panel, a bottom panel or a
  window. A part is drawn from a tree of the window's own widgets that the plugin's
  program sends, including a live screen that takes taps and swipes. Pinned parts show
  their icon; the rest are under a Plugins button with search and folding groups, and
  panels open as tabs of one dock, so many plugins stay calm.
- Plugin isolation: every program a plugin runs starts with an empty environment, under
  the system's sandbox, without access to the config folder, other plugins or the
  keychain. It reaches the network only through the core's broker and only to the
  origins its manifest lists; a plugin's own key is put into its requests by the core
  and never seen by the plugin unless it is marked handed. The permissions are shown
  before a plugin is switched on, and an update that asks for more turns it off.
- A plugin store: the core lists the plugins of a signed `index.json` and installs one
  after checking the signature, the package's hash and size, every path in it and that
  its manifest asks for what the list showed. A browser may look but not install.
- A Workshop under Plugins in the desktop app and the terminal (`w` in `/plugins`, or
  `/plugins store`) to install, update and remove plugins of the store in one step.
- The chat shows which model each subagent runs on, in the Subagents window, its chat
  and the terminal's side panel.
- A sound file named in a message (mp3, wav, ogg, flac, m4a) shows as a card that plays
  in the desktop app; the model still gets its path.
- The limits ring for Sign in with ChatGPT: the 5-hour and weekly windows of the plan,
  read from the ChatGPT backend's own headers.
- While the agent works, typing shows the Send button beside a small Stop, so a message
  can join the turn by click as well as by Enter.
- `platforms` and `clients` in `plugin.yml`: the systems a plugin is built for and the
  clients that can draw its parts.

### Changed

- The `secrets` field of `plugin.yml` is now `permissions.keys`, with the origins each
  key may go to. The built-in Telegram plugin asks for api.telegram.org only and is off
  after this update until you switch it on again. On Linux, plugins that need the
  network do not start, since Landlock cannot hold them to the broker.
- Telegram is no longer in the binary: it is a plugin of the store, installed from
  the Workshop (macOS for now). The copy older versions put in the config folder is
  removed at start unless you edited it.

## 0.74.0 - 2026-11-14

### Added

- While Sign in with ChatGPT waits for the browser, Copy URL takes the page to another
  browser; the terminal prints the address too.
- A ring over the context ring shows how much of its rate limits the current model's
  account has used, when the provider reports them.
- In the connect form the fields follow the provider: Sign in with ChatGPT shows under
  OpenAI, and an address is asked only of a custom server.
- More models has a button that asks the providers for their models again.
- The iOS Simulator in the desktop window on macOS: a phone in the header opens it at
  the right edge. It walks through the setup (Xcode, the iOS runtime, a device) and
  downloads the runtime on request. The device is drawn as a phone with its screen kept
  current; a menu at the top chooses another device. A click on the screen is a tap and
  a drag a swipe; buttons under it press home and lock, open Simulator.app and shut the
  device down. Touches and keys go through AXe, a free command line tool the panel says
  how to install. The agent gets an `ios_simulator` tool on macOS: a picture of the
  screen in points, its elements as an accessibility tree, taps, swipes, typing,
  buttons, links and installed apps on the booted device. The tree is cut down to one line
  for each labelled element with the point to tap: a home screen goes from about 400 KB of
  JSON to under 2 KB. The core runs simctl and AXe
  without a shell and names a device only by a UDID that parses as one; the web client
  cannot ask for the simulator. A header that was never reshaped takes the new button.
- A page for the project on GitHub Pages, `site/index.html`, published by its own
  workflow with the readme's screenshots. No scripts or fonts from other hosts.
- The readme shows the three clients, says how a model is connected and how sign-in
  with ChatGPT works, and names a way to say thanks.
- Sign in with ChatGPT: OpenAI connects without an API key. The browser asks the user,
  OpenAI issues this install a client ID of its own, and the plan's models run over the
  Responses API. It is in the provider settings of the desktop window and first in
  `/connect` of the terminal client. The identity token's signature, issuer, audience,
  expiry and nonce are checked; the record is kept where API keys are kept, and its
  rotating refresh token is saved after every renewal and read again before one, so
  the desktop and terminal clients can run side by side. A record that cannot be read
  leaves the other providers working. A new provider kind, `chatgpt`,
  cannot be made from a form or pointed at another address. New direct dependency:
  `aws-lc-rs`, already in the tree under rustls.

### Changed

- The crates are laid out by role: `projects/server` (was `web`), `projects/clients/`
  `terminal`, `desktop` and `browser` (were `tui`, `gui` and `web-ui`),
  `projects/plugins/telegram` (was `external/telegram`), and `projects/evals` with its
  tasks. The crates are named after their places (`dusk_server`, `dusk_terminal`,
  `dusk_desktop`, `dusk_browser`); the programs keep their names.
- Inside the crates, each file has one reason to change and each module a documented
  door: the core's helpers without a domain went to the domains that use them, the
  thread's runtime is one `runtime/thread` module, large files (the desktop's chat
  messages, composer and workspace, the terminal's actions and sidebar, the core's
  sources and platforms) became directories, and the Telegram bot is grouped by feature
  rather than by layer. `scripts/structure-map.sh` regenerates the map in
  `docs/structure.md`.
- Logic the clients each wrote for themselves has one home: schedule text is parsed in
  the protocol, one WAV encoder serves the desktop and the browser, the core reads the
  terminal's git line and decides what an uninstall removes, and the tool and agent
  state the desktop and the browser both show is shared.
- A temporary chat is marked by a speech bubble drawn in dashes, the sign most apps use
  for it, in place of the hourglass.

### Fixed

- A turn whose model call failed showed as finished with no word of why; it now says
  the error. Behind one such case, ChatGPT answers that are not marked as an event stream
  are read as one, and a JSON answer gives the server's own message.
- Sign in with ChatGPT refused an identity whose audience lists this app among others
  without naming an authorized party; the check now follows OpenID Connect, and a refusal
  logs the audience it saw.
- The iOS Simulator panel was slow and its screen took no clicks: frames now come as a
  stream from AXe, a click on the screen is a tap and a drag a swipe, and a device that
  is booting is waited for instead of failing with "Error creating the image".
- Web: a turn that only thought and answered, with no tool, drew its "Worked for" line
  twice and never showed the answer. The line's fold and the answer had the same key in
  the list, since the fold began with the answer's own thinking.
- Web: signing in again no longer leaves the earlier stream to the core open beside the
  new one.

## 0.73.5 - 2026-11-13

### Added

- GUI: the first start after an update opens a window that says what the update
  brought: how many things are new, changed and fixed, and each of them, read from the
  changelog built into the program. It shows once for a version.
- The web client can be opened to other devices: `duskcode --web --global`, or the
  "Other devices" switch beside Web access in the desktop window. The address shown is
  then the machine's address on its network. A server that listens on every address
  now takes a request addressed to any of the machine's addresses written as a number;
  a request for a name is still refused unless that name was allowed, so DNS rebinding
  stays closed. The traffic is not encrypted, and both clients say so.

### Fixed

- Releases for macOS: the terminal client is signed by the same certificate as the
  desktop app, when the repository holds one. Without it both are signed ad hoc and
  macOS asks for keychain access again after every update; the release job now says so.
- An agent or skill whose front matter is not valid YAML says why, and names the usual
  cause: a value that holds a colon and a space without quotes.
- GUI: the header's sky stays up while subagents of the chat still work after its turn.
- GUI: the subagents window no longer opens with its close button drawn as hovered.

### Changed

- TUI: a sky over the thread. A few stars stand on the header's rows and above the
  horizon of a new thread; they twinkle while a turn runs, when now and then one falls,
  and stand still otherwise, so an idle terminal is not redrawn.
- TUI: the rail reads as a dashboard. The model and its mode sit on a card edged in the
  mode's colour, each section starts under a hairline, the context gauge warms toward
  its end and marks where compaction starts, and a task shows how far it is as a bar.
- TUI: a step carries the mark of its kind: a prompt for a command, lines for reading,
  plus and minus for an edit, and so on. Its colour still says how the step stands.
- GUI: small falling stars are short white scratches that fall at any lean, as the first
  ones did, only smooth. The great flaming one stays rare.

## 0.73.4 - 2026-11-13

### Added

- A file open in the desktop window can be changed and saved. The core writes it whole and
  in one step, only inside a saved project or the chat folders, keeps its permissions, and
  refuses the save when the file changed on disk since it was opened. Browsers cannot
  write files this way.
- Terminals in the desktop window: the user's own login shell on a pseudo-terminal the
  core runs, started only inside a saved project or the workspace. A terminal belongs to
  the client that opened it and ends with it; the agents cannot reach it, nothing typed or
  printed is stored or logged, and browsers are refused. New dependency: `portable-pty`.
- GUI: the terminal is a panel under the chat with tabs, opened from the rail or with
  cmd-J; the file page saves with cmd-S and warns before unsaved text is dropped. New
  dependency: `alacritty_terminal`.

- Temporary chats: a chat that is never written to disk. The core keeps it in a database
  in memory, its files and artifacts in a private folder of the system's temporary
  folder, and it writes no memory; all of it is gone when the core stops, and a folder a
  crash left is removed at the next start. It is listed while it lives, is not found by
  the search and is in no backup. An hourglass beside New chat in the desktop window,
  `/temp` in every client, a button in the browser. Sounds made by reading aloud are
  still kept among the stored artifacts.
- Search in what was said: the core finds messages of any chat by their text, and the
  desktop search window lists them under the chats; a found message opens its chat at
  that message. Cmd-F opens the same window as Cmd-K.
- GUI: a background job opens in a window with its whole command, how long it runs, its
  output so far and Cancel; the agent is told the job was cancelled.
- GUI: a copy button on every kind of message, the folded ones too: thoughts, reports,
  a step's arguments and answer, commands, and a subagent's chat.
- GUI: a switch for the fireflies under Appearance.

### Changed

- Several questions asked in a row wait in a queue in every client and are answered by
  one message; before, the second took the place of the first.
- GUI: the settings menu is grouped as Agent, Window, Data and System, and the context
  window stands on the Model page with the size in force.
- GUI: the todo list is cards with a progress line and the item in work lit; a loaded
  skill shows by its name among the steps; More models has no heading of its own beside
  the menu.

- GUI: the falling star is one smooth stroke of light on a shallow path, white at the
  head and blue behind, gone in a quarter of a second; one in a few crosses the whole
  sky. It has the shape of a flame: a drop with a rounded nose that narrows without a
  waist into the thin trail, white-hot inside and blue at the skin.
- GUI: small falling stars are the usual ones again; a great one is rare, crosses the
  whole sky in an instant and leaves its trail hanging for a moment before it goes out.
- GUI: an hourglass in the rail starts a temporary chat.
- GUI: rings go out from the microphone while it records.
- GUI: the arbiter window anew: the agents as a tree with their roles, the arbiter's
  state as a mark over one plate of counts, and what passed as a timeline in plain
  words with the time since.
- GUI: Quiet's moon is a crescent, a disc covered by a shadow of its own size.

### Fixed

- A model whose provider did not answer at the start stayed at the unknown window of
  128 000 tokens until it was chosen again. The start asks again, and any catalogue read
  later fills the window in.
- GUI: a message picked in the chat's list of messages is shown also when the chat
  stood at its end; before, the list stayed where it was.
- GUI: one zone of subagents takes the whole window instead of half of it, and a long
  brief no longer runs down the row.

## 0.73.3 - 2026-11-13

### Added

- A provider can be switched off in the model settings of every client: it keeps its key
  but is not asked, its models are not offered and discovery skips it. The config says
  `enabled: false` on it. The provider of the chosen model cannot be switched off.
- Google AI as a kind of provider (`kind: google`): the Gemini API with its own model
  catalogue, streamed answers, thought summaries, function calls and picture input.
- The model settings of every client show a provider's rate limits when its answers
  report them (OpenAI-compatible `x-ratelimit-*` and Anthropic `anthropic-ratelimit-*`
  headers): requests and tokens left of the limit, and when they are full again. Read off
  the answers that come anyway and kept in memory only.
- GUI: slash commands in the message box. Typing `/` lists them, Tab or a click completes
  one; commands that only show something open the window's own place for it.
- GUI: a new chat opens on a line in a warm serif that changes with the time of day.
- `domain-structure` skill: laying out and auditing any codebase as domains with front
  doors and one reason to change per file; `docs/structure-audit.md` lists what here does
  not fit yet.
- GUI: a Sources module in the side panel lists what is attached to the chat; each
  source goes to the model once, and a switch on it sends it with every request.
- GUI: a falling star crosses the header's sky now and then, and mist rises at the foot
  of a window left alone for a few minutes; a click or a key clears it. Appearance has a
  switch for each.

- GUI: a long unfolded block (a turn's activity, thinking, a step, a run of steps) ends
  with its heading again, so it folds from where the reading ended.
- GUI: a firefly glows over the message box for each subagent at work; a turn that runs
  past sunset sees the header's glow go on into a deep blue night; a turn that ends
  while the mist stands lifts it with a warm light along the foot of the window.
- GUI: Memory and context has the context window and the sizes of the memory and of the
  work history.

- GUI: a step's icon says what it is: the terminal, a file, a search, an edit, the web,
  another agent, a connector's tool; thinking and reports have their own. Its color says
  how it stands.
- GUI: the context ring breathes once the context is more than 85% full.
- GUI: a subagent's chat shows its task and reports as markdown, its thinking folded, its
  errors apart, and who it is at the top; one agent can be stopped from its row or its
  chat, and finished ones cleared from the list, one by one or all at once.

### Changed

- The context report and a thread's `Tokens` events count with the same tools and model,
  so Statistics and the ring by the message box agree; Statistics follows the turn.
- GUI: the quiet mode's moon is filled while it is on; Connectors stands above Artifacts
  in the rail; the sky's stars are soft points with short rays and a rare tinted one;
  More models names each provider in bold and folds it.
- GUI, Dusk: a chosen value in a row of choices is tinted with the accent instead of
  filled with it; the settings page is wider and stands in the middle; the frame is a
  step lighter and the quiet text a step brighter; the message box lights up a little
  while it is written in.

### Fixed

- GUI: the wheel over the font list in Appearance scrolls the list, not the page.
- GUI: a long last answer can be scrolled down again after scrolling up in it.
- GUI: background jobs show in the Jobs module as they start and end.

## 0.73.2 - 2026-11-12

### Added

- Desktop: while a turn runs, the header carries a faint sky that drifts from violet to a
  sunset orange as the turn goes on, and a star lights up once when it ends. Each turn
  says what it spent, tokens and cost, marked when it was more than twice the chat's
  usual. A moon on the rail turns on quiet mode: only the chat and its box, the header
  thinned to a line. A "while you were away" card lists the scheduled tasks that ran
  since the last look, each with a way to its chat. The subagents window leads with each
  agent's task, then what it does now in words, its steps and time, and its last remark.
- Stars come out in the header's sky one by one as a turn runs on, each twinkling on its
  own beat; Appearance can switch the sky off. The rail, the header's tools and the parts
  around the message box are drawn from one layout map, which `gui.json` may override to
  reorder or trim them.

### Changed

- A scheduled task can name its model; the desktop editor's lists keep their clicks
  instead of closing the window. While a turn runs, what the agent says stays in sight
  and only its steps fold; once it is over, everything but its answer folds, in every
  client, and says what each tool did in plain words, work for agents in the
  accent. Desktop composer: attach and access inside the box with the microphone as
  large as Send, the mode and the model under it, the context as a percentage beside it
  with the counts on hover. Answers read as text, their tools on hover; the review button
  shows the git mark; menus show which row is under the pointer; the side panel is
  narrower with inset rules and larger headers; the wordmark sits over the sidebar; the
  rail previews a place only while the sidebar is hidden; Settings, Chat opens the mode
  list; text defaults to 16 px. The composer stands between two columns of one width,
  the message history on the left and the context ring with its share on the right, level
  with the box's buttons; the microphone is a plain icon. A New chat above the projects
  starts one that belongs to none.
- Grid and motion: every size in the desktop app comes from one scale on a 4 px grid
  (controls 32, rows 36, bars 52, icons 16 and 20); menus and windows rise into place in
  160 ms, folds slide open in 180 ms. The web client takes the same grid and motion as
  CSS variables, honours reduced motion, and gets the desktop composer: access inside the
  box, mode and a short model name under it, the context as a ring with its share.

## 0.73.1 - 2026-11-12

### Fixed

- The desktop app shows the agent's own cursor where it clicks: run as the cursor helper,
  the app now becomes the helper instead of opening a second window.

### Added

- Terminal: /schedules lists the scheduled tasks to run now, switch or remove, and
  /schedule daily 09:00 <prompt> (or weekdays, weekends, weekly mon 09:00, every 2h,
  once 14:30) adds one in this folder's project. The model list selects providers too:
  c connects one, r checks again, d twice removes it; Settings, models opens it. The
  footer shows how long the turn has run, and the chat says how long it took.
- Web client sends pictures: picked, pasted or dropped, shown as thumbnails until sent.
  The core keeps a PNG or JPEG of up to 6 MiB a client sends as bytes among the chat's
  sources. The web transcript folds each turn's steps into one line that says how long
  the agent worked, as the desktop app does.
- Scheduled tasks: a prompt the core sends to a new chat of a saved project once, every
  so many minutes or hours, every day, on weekdays or weekends, every week or on chosen
  days. The desktop app lists them on a page of their own with Run now, a switch and an
  editor, and so does the web client; a run nobody watches goes on to the end of its turn.
  Protocol version 10.

### Changed

- Desktop chrome: a header across the whole window holds, over the rail and sidebar,
  their switches and Back and Forward, the chat's title past a thin rule. The rail is a
  shade darker than the sidebar, the sidebar is a card with one rounded corner, and the
  wordmark made room for the controls.
- The side panel lists screenshots as rows with a small thumbnail, the first five with
  "View all" for the rest. Click and scroll tools tell the model to take the user's own
  pointer only when the user asked for it.
- Desktop: a wider rail with larger icons that can itself be hidden; resting on its Chats
  or Scheduled icon shows that place beside it, a click opens it. The top bar has Back and
  Forward through the places visited, the chat's project on hover of its folder, and a
  "..." with the chat's menu, which can also schedule a task. The work area is one rounded
  sheet, the user's messages carry a light accent, and each turn folds its steps, thinking
  and passing remarks into one row that says how long the agent worked.
- Updates page installs a newer desktop app in place and offers the restart into it,
  instead of opening the release page. Releases are signed by one certificate when the
  release secrets hold it, so macOS keeps permissions and keychain access across
  versions; an app signed that way takes only an update signed by the same certificate.
- Web client builds install `wasm-bindgen-cli` from source, so Linux runners with an
  older glibc build it too.
- Desktop layout: a thin rail at the left edge holds the places of the app, the sidebar
  holds only projects and chats, a 52 px bar runs across the top to the right edge with
  the side panel under it, both side columns slide open and closed, and the side
  panel's foot (project, folder, branch, model, mode) starts folded behind an arrow.
- Desktop and web look: a deeper Dusk palette, settings grouped on cards as macOS lists
  them, controls at least 32 px (steppers, switches, buttons), the web address in a
  copy field that never leaves its card, System usage as tiles, and clearer composer
  controls with menu carets and the model's name.
- Desktop chat groups each turn's tool calls and reasoning under a collapsed row labeled
  with the latest action. Assistant messages and prompts stay visible.
- The composer has a clearer message history button, and the Subagents panel fits
  smaller windows and returns keyboard focus to its button on close.

## 0.73.0 - 2026-11-11

### Added

- The web client: the chats, projects, files, review, connectors (MCP, skills, plugins,
  tools, agents, voice) and settings of the desktop app in a browser, written in Rust with
  Leptos and built with trunk (`scripts/build-web.sh`). `duskcode --web [--port N]` (default 8080) serves it
  beside the terminal client, and Settings > Advanced > Web access does the same from the
  desktop app. It listens on 127.0.0.1; the address printed once carries a token of this run
  that is traded for a session cookie. See `docs/web.md`.
- One core, several clients: the web, the desktop and the terminal clients can be attached to
  the same running core and see the same chats live. A chat closes only when the last client
  that opened it leaves it.
- Protocol: `RequestFrame` and `ServerFrame` carry requests, responses, events and status
  over a message stream; `Status.workspace_dir` says where a chat without a project works
  (still protocol 9).

### Changed

- Desktop chat gives tool actions and their status a clearer, more compact layout.
- Message history shows the current position and lists sent messages without background job
  results.
- The Models menu highlights the current model, shows full names on hover and adapts its
  submenus to the space beside the composer.

## 0.72.7 - 2026-11-11 (pre-release)

### Added

- Voice: speech models that run on this machine, installed with one click. On Apple Silicon:
  Qwen3-ASR, Whisper large-v3-turbo (recognition), Qwen3-TTS and VoxCPM2 (reading aloud) on
  the GPU with MLX, and Kokoro 82M; elsewhere Whisper and Kokoro on the CPU. The app asks `uv`
  for a Python environment under `~/.local/share/duskcode/voice/`, installs the pinned
  packages, downloads the weights (with the Hugging Face token, if set), starts
  `resources/voice/server.py` on a loopback port behind a token made for each run, points the
  speech settings at it, and starts it again with the app. Remove deletes the model and what
  it downloaded. `uv` must be installed; the page says so (protocol 8).

### Changed

- The Voice shelf follows the style of Settings: cards, large labeled fields, tiles.
- Reading aloud asks the server for wav, not mp3: any server of the audio API can make it.

## 0.72.6 - 2026-11-11 (pre-release)

### Changed

- Desktop composer menus (model, mode, access) and the queue follow one keyboard and visual
  rhythm; the connectors graph and cards are tidier.
- The artifacts gallery shows one entry per managed image with a bounded thumbnail; file
  actions still use the original. Outputs carry a preview (`OutputPreview`, optional field).

## 0.72.5 - 2026-11-11 (pre-release)

### Changed

- README: a section on where keys and tokens are kept, what leaves the machine, and how to
  build the project yourself.
- Desktop settings give the Hugging Face token, credentials, profile, snippets, themes and
  fonts clearer layouts and larger controls while keeping the Dusk palette.
- macOS window sessions capture only the selected window, including when another window
  covers it. Regions and click coordinates stay tied to the selected window.
- Subagent foxes stay in place as their eyes and tools animate. Completion and error effects
  play once, and tool details stay inside the sprite.

## 0.72.4 - 2026-11-11 (pre-release)

### Added

- The desktop client ships for Linux as an AppImage, for amd64 and arm64, built by the release
  workflow; `install-gui.sh` installs it. The window's libraries are the machine's own.
- Settings, Model: a larger connect card with the kinds as tiles, labeled fields and a Check
  connection button that asks the server for its models without saving; each saved provider has
  Check, Reconnect (anew, with a new key if one is typed) and Remove, and the list has Check
  all (protocol 7).

### Changed

- The programs find their resources in `share/duskcode/resources` beside `bin` on Linux, as
  they do inside the app on macOS.

### Fixed

- The keychain asked for the password for each key at every start, and again after an update:
  every secret was its own item. All secrets are now one item, read once per run; the old items
  move into it the first time and are deleted.
- Cmd+Q did nothing in the desktop client; it quits now, and the application menu says so.

## 0.72.3 - 2026-11-11

Pre-release: speech through any OpenAI-compatible server, and the whole code tree laid out
by domain.

### Added

- Voice: speech through any server that speaks the OpenAI audio API, a hosted account or a model
  run on this machine. Connectors, Voice sets the recognition and the synthesis server (keys in
  the keychain, protocol 6). A microphone button beside Models dictates into the message box,
  cut at pauses and sent piece by piece; a Listen button under each answer reads it aloud and
  plays it. Downloading models from Hugging Face and running them inside the app are not done.

### Changed

- gui: a new app icon, 1024 px with transparent corners.
- The code is laid out by domain, with no change in behavior: the protocol is ten domains with
  submodules, the core's secrets, voice, client, runtime, agent and config are directory
  modules, the GUI is pages, columns, ui and platform, and the biggest TUI files are split by
  what they draw. See docs/structure.md.

## 0.72.2 - 2026-11-11

Pre-release: secrets in the keychain, sound cards, self-update, uninstall and the new composer.

### Added

- `display_audio`: the agent can put a sound in the chat as a card with a play button, a bar
  that takes a click to jump, the time and a menu for the file. MP3, WAV, OGG, FLAC and M4A, up
  to 32 MB, recognized by their first bytes. The picture record `ImageArtifact` is now `Card`
  and carries a kind and a duration; old records read as pictures. Telegram gets no sounds yet.
- Secrets in the system keychain (macOS Keychain, Secret Service, Credential Manager). Provider
  API keys move out of config.yml on the first start and are read back into memory; if the
  keychain refuses, they stay in the file. Settings, Secrets lists what is set, never a
  value, and keeps the Hugging Face token (protocol 5). `duskcode remove` clears the
  keychain entries; a /migrate archive no longer carries the keys.
- gui: the desktop app finds a newer release, downloads its disk image in the background,
  checks it against GitHub's digest, swaps the app bundle in place and offers "Restart to
  update". It never gets the quarantine mark, so macOS does not ask again.
- gui: Settings, Advanced, Remove duskcode: three steps and a typed word, then
  `duskcode remove --yes` runs in the background after the window closes. Chat files in
  ~/Documents/duskcode stay unless asked for.
- gui: a single Remove all button for the connected providers (protocol 4).
- gui: files and pictures dropped on the chat go with the next message; attachments show as a
  row of thumbnails above the text that scrolls sideways.
- install-gui.sh on a Mac installs the released app with curl, without the quarantine mark.

### Changed

- gui: the composer is one card: the text across its whole width, and a row below with
  the mode, the access, the context meter, a Models button and Send. The Models button opens
  the models and the effort upward. Headers of the side column have icons.

### Fixed

- A provider connected in the desktop app answered 401 when its pasted key carried a
  trailing newline or space; the core now trims the key.

## 0.72.1 - 2026-11-10

Pre-release: GUI work since 0.72.0.

### Changed

- gui: copy and go-back buttons on your messages (going back asks first and erases the message
  and all after it); a Stop button while subagents work; Back in a subagent's chat; the
  subagent fox stands centered; Run on a shell block starts at once and shows its output in
  the chat under a filling bar; artifacts are rounded square tiles with a right-click menu.

## 0.72.0 - 2026-11-10

Full release after an end-to-end run on qwen3.8-flash (6 tasks, 2 runs each: 8 of 12 passed, 0.6
tool errors a run, no regression against the earlier run of 7 of 12).

### Changed

- Subagents in both clients from one source: the kind of work a role belongs to (engineering,
  security, testing and review, infrastructure, general) and the word for what an agent does
  now (thinking, writing, searching, reading, planning, running) are decided in
  `dusk_protocol::agent`, not in a client. The GUI stage and the TUI side card both group by
  them; the TUI card also shows the action word while an agent works.
- GUI: the subagents' figure is a fox, the animal of the dusk, down to the chest and as simple as
  8-bit gets: 16 by 16 cells, symmetric by construction, two ears, a pointed face with a small
  cream muzzle. It thinks with eyes turned up and dots that fill, writes with a pencil, hammers
  for a command, holds a lens for a search and a page for a read, closes its eyes with sparks when
  done, and is dizzy under stars when it failed.
- GUI Connectors: a title and a line about the page, the shelves as one segmented control with
  the search on the same row, cards on the type scale with rounder corners, empty shelves say
  what to do, the Agents shelf shows its trail only once an agent is entered, and the window of
  a skill lines its facts up in one column and reads at the interface's size.
- Rules about running commands, reading files, asking agents, asking the user and the task
  board are in each tool's description instead of the MAIN prompt (about 160 tokens fewer
  per request); no measurable change in the evals.
- GUI menus and windows: the search window shows the source of a chat as quiet text, not a chip,
  and asks for a title; the chat menu sets Delete apart; the effort menu uses the same text size
  as the menu it opens from.
- GUI: the subagents moved out of the right column to a button beside the message box: the
  figure of the agent at work (or the latest), a count, a ring in the signal color while any
  works. It opens a stage with a zone for each kind of work (engineering, security, testing and
  review, infrastructure, general); every agent is its 8-bit figure with a bubble for what it
  does now: thinking (breathing), the tool it runs, writing, done, failed. The screenshot
  script can stage agents (`fake=agents`, `modal=agents`).
- GUI chat: answers have a taller line, the type scale is used all through, the model in the bar
  is shown without its provider and cut to a length the bar holds (the full name in the tip), a
  command waiting for approval is shown as code.
- GUI design system (`ui.rs`): one type scale in rems (meta, small, body, title, group name), a
  4 px spacing grid, three motion times, and shared pieces: group names, page titles, a turning
  ring for every wait, empty states with an icon and a next step. Applied to the sidebar,
  module headers, settings (each page has a title and a line saying what it is for), the review
  and the connector tabs. The steps of a chat run sit on a rail, the running one in the
  signal color and the finished ones receding. Chat source is quiet text, none for the GUI.
  Pages about the app itself (settings, connectors, artifacts) use the full width: the side
  column shows only with a chat, the review or a file.
- GUI, from a pass over every page in pictures: card descriptions end in an ellipsis instead of
  being cut mid-letter; the "+N" of an agent's links is no longer cut off; the Changes module
  has no second "Changes" heading; the context legend uses six fixed hues, so "tools" and
  "tool results" no longer look alike; the Model page's section labels line up with their
  rows; font rows have one height; the hunk header in the review has room around it.
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

- GUI keyboard: shortcuts for a new chat (⌘N), finding a chat (⌘K), settings (⌘,), the sidebar
  (⌘B) and the side panel (⌘⌥B), Ctrl instead of ⌘ off a Mac; they are listed on a new Keyboard
  settings page. Escape closes what is on top, one thing a press: a window, a menu, the review;
  with nothing open it leaves the message box, and then Tab and Shift+Tab walk the rows. A row
  in focus has a ring in the accent color, and Enter or Space presses it.
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

- GUI: the menu of a chat (right click) and the menu of the project path in the right column
  stopped the window with a panic, "hover style already set": a row of a menu was given its
  pointer color twice. Menu rows now come from `widgets::menu_item`.

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
