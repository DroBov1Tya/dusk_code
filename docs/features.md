# Features and their switches

What the core does beyond the basic turn, and how to switch it. Switches live in
`~/.config/duskcode/config.yml`: `limits.*` under `limits:`, tools under `tools:`. A tool
named in `OFF_UNTIL_SWITCHED_ON` (`projects/core/src/config/file/switches.rs`) is off in
every mode until `tools:` names it. Defaults are in `projects/core/src/config/sections.rs`.

## Provider failures and resume

- **Retry row.** A model call that is tried again shows as one row that updates in place
  with a countdown; a failed turn ends with one error that names the kind of failure and the
  attempts. `limits.max_attempts` (6) bounds the tries. A host that cannot be reached is
  tried for `limits.retry_unreachable_seconds` (300) instead. The settings show whether each
  provider is answering or failing.
- **Provider waits.** `limits.wait_for_provider` (on): a turn whose provider is rate
  limited, out of quota, unreachable or overloaded waits and goes on by itself, for up to a
  day. The wait is stored and survives a restart; the user's stop or message ends it.
  `limits.shrink_context` (on): a compaction whose own request is too large for the model
  summarizes only the older half.
- **Resume after a crash.** A turn cut off by a restart goes on where it stopped. A call
  that may have taken effect is never repeated without asking (Continue, Check first,
  Stop). A turn idle for more than `limits.resume_within_seconds` (3600) asks first.

## Changes and context

- **Change ledger.** `limits.record_changes` (on): the files each command changes in its
  project are recorded (sizes, hash, a capped diff when the earlier text was read; never
  for `.env*` or `.ssh`). It feeds the chat's list of changed files. The read-only
  `changes` tool asks it by path, time and thread; `tools: { changes: false }` hides it.
- **Result stubs.** `limits.stub_results` (on): old results of `run_command`,
  `read_file`, `search_files` and `read_history` are sent as a one-line stub that names the
  `read_history {"result": N}` call returning them, when that pays for the cache it breaks.
  Results of the newest `limits.stub_keep_steps` (8) steps and results under
  `limits.stub_min_tokens` (200) stay whole. The full text stays stored.
- **Tail compaction.** `limits.compact_tail` (on): a summary covers the older part; the
  newest messages, up to `limits.compact_tail_tokens` (16,000), stay word for word.
  `limits.compact_early` (on) also compacts between turns past 60% of the input limit.
- **Stable prefix.** `limits.stable_prefix` (on): each tool description stays as the
  thread first showed it until the next compaction, so memory or skill changes do not
  break the provider's cache.
- **Reading several files.** `limits.read_many_files` (on): `read_file` also takes
  `paths` (2 to 8 files in one call). `limits.parallel_reads` (on): the read-only calls of
  one step run together, at most 8.
- **Shaped command output.** `limits.shape_output` (on): the output of `cargo`,
  `pytest`, `npm`/`pnpm`/`yarn`, `tsc` and git transfers is sent without progress and
  passing tests, repeated lines folded, with a line saying how many were left out. Errors,
  warnings and `file:line` lines stay; piped or unknown commands are not shaped. The whole
  output stays stored and `read_history {"result": N}` returns it.
- **Failure ladder.** `limits.failure_ladder` (on): when the same error returns within a
  turn through calls that differ, the model is told to stop varying the call at the third
  time (`limits.failure_note_at`), asked for a different plan at the fifth
  (`failure_replan_at`), and the turn ends as failed at the eighth (`failure_stop_at`).
- **Loop cycles.** `limits.loop_cycles` (on): within the last 20 calls of a turn, a
  short run of calls (one to four) coming round with the same results, or a read giving
  the same result with nothing that may write between, gets a note at the third round
  (`limits.cycle_note_at`) and a request for a different plan at the fifth
  (`cycle_replan_at`). The reads of one step count as one; ids, times and durations in
  results do not hide a repeat; a read again counts only within the steps whose results
  are still sent whole (`stub_keep_steps`). Notes only. Off, the same call with the same
  result three times in a row still gets a note, as before.
- **Test re-runs.** `limits.test_reruns` (on): a test run (`cargo test`, `pytest`, `go
  test`, `npm test`, `jest`, ...) that repeats a run of the same runner with no project
  file changed since, as the same command or with the same tests failing, gets a note
  with the step of that run and its failing tests (`limits.test_rerun_note_at`, 2), and a
  request for a different plan at the fourth such run (`test_rerun_replan_at`). A
  narrower run to look at one failure is not a repeat. Changes are read from the change
  ledger once it has recorded one in the chat, else any call that may write counts as
  one; a write the ledger cannot see (a subagent, a background job, an MCP tool) always
  counts. The command still runs. The failing test ids also stand for a run's
  error in the failure ladder, so the same failures under another filter climb it.

## Project knowledge

- **File map.** `limits.project_graph` (on): the `project_graph` tool (`tools: { project_graph: false }`
  hides it) maps the working folder by file and folder names, never their content: an
  indented tree that folds deeper folders to `name/ (N files)` to stay under 300 lines and
  12 KiB, one folder of it on `path`, or the paths matching `find` (a part of a path or a
  glob). It walks as the change ledger does (ignore rules, no `.git`, no links followed) on
  every call; nothing is indexed or stored. The name is kept from the code graph it replaced.
- **Episodes and `recall`.** `limits.episodes` (on): for a saved project, what ended a
  repeated failure, or a goal's verification that passed, is stored as an episode (at most
  500 a project). `limits.episode_lesson` (on) adds a one-line lesson from the model. The
  `recall` tool (`tools: { recall: false }` hides it) returns up to three notes.

## What the chat shows

- **Structured blocks.** The `show` tool puts a flow, a list of checks or reply buttons in
  the chat; on by default; `tools: { show: false }` hides it. See [blocks.md](blocks.md).
- **Overview.** The Statistics view of every client: the plan's progress and members,
  tokens, context, changes, tool activity and an event log. The core counts all of it;
  there is no switch.
- **Working row.** While a turn runs, the chat shows time, tokens and step, for example
  `Working · 2m 14s · 18.4k tokens · step 7`, counted by the core; no switch.

## Teams (TEAM mode)

A team runs only in the chat's TEAM mode; every other mode keeps the board and the agents
`ask_agent` starts. A chat's mode is the one the user last wrote to it in: a turn the core
starts there, such as for a line from its team, keeps it, so switching the mode for
another chat does not touch this one. `limits.plan_graph` (on) is the kill switch: off, TEAM mode has the
board too and no team runs.

- **The plan.** In TEAM mode the lead's `tasks` tool takes a plan: tasks that depend on
  each other, each with its files, role, acceptance and gate. `run` has the core give each
  ready task to a member (a thread under the chat) and keep a lease on it. Once the user
  writes to the chat in another mode, nothing new is given out; what is at work finishes,
  and the team goes on when the chat is back in TEAM mode.
- **Members.** A member gets a card the core writes: the task, its files, how done is
  told, what was done before it, and the rules. Its tools are an allowlist (`run_command`,
  `jobs`, `read_file`, `search_files`, `project_graph`, `recall`, `read_history`,
  `load_skill`); a role's own `tools` and `team: { tools: { <role>: [..] } }` only take
  from it. Its approval mode is the chat's, read again before each step. It answers on
  `team: { models: { <role>: provider/model } }`, else `team.model`, else the chat's
  model; a role set to a provider the config lacks fails its task with that reason.
- **Gates.** A command gate is run by the core in the project folder under the chat's
  approval mode: exit 0 makes the task done; anything else sends it back to its member
  with the end of the output, twice at most, then the task fails and the lead decides. In
  a mode that would ask the user, `run` asks once, in the usual approval card, with each
  distinct gate command listed: a yes lets exactly those lines run as this team's gates
  (in memory, like a yes for the session); a gate added or changed later, or one after a
  restart, is not run and its task waits for the lead until a `run` asks again; a no
  leaves every gated task to the lead. A gate runs `limits.team_gate_seconds` at most
  (300), a team runs `limits.team_max_gate_runs` gates (128), and an end of the team
  kills the gates that run; their tasks stay in review for the next run. A task without
  a gate waits for the lead's review. A task whose gate is the user's
  confirmation waits in review while the team goes on; the lead is told once, and the
  desktop plan row has `confirm`.
- **Files.** At hand-in the change ledger's rows of the member's turns on the task are
  compared with its `files`; a change outside them holds the task for the lead with the
  paths named, and its gate does not run. The ledger walks with the ignore rules, so
  build output never counts. With members at work at once a walk around one command also
  sees another's writes, so a path another thread recorded meanwhile is left out: the
  check holds only what is surely the member's. A change the user or a background job
  made during a member's command counts as the member's. A task without `files` is not
  checked.
- **What is forwarded.** A line to the lead (400 bytes at most), a task's note, a gate's
  output and a card are made plain and cleared of the project's variables and of what is
  shaped like a key before they are stored or sent.
- **Budgets and ends.** `limits.team_max_steps`, `team_max_tokens`, `team_max_minutes`
  and `team_at_once` are the ceilings; a member takes no step once one is spent. The team
  ends when all is done, a budget is spent, it is stopped, or nothing can start and the
  lead changed nothing.
- **Recovery.** When the chat is opened after a restart, an ended team's work goes back
  to open, a turn still running gets its lease again, a turn that completed is handed in,
  a lost one is released, and a gate that was cut off runs again.

Not built: a worktree per member, messages between members, procedures (their switches
`limits.member_worktrees`, `peer_messages`, `procedures` do nothing yet; `task_packets`
is no longer read).
