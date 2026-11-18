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
  `limits.shrink_context` (off): a compaction whose own request is too large for the model
  summarizes only the older half.
- **Resume after a crash.** A turn cut off by a restart goes on where it stopped. A call
  that may have taken effect is never repeated without asking (Continue, Check first,
  Stop). A turn idle for more than `limits.resume_within_seconds` (3600) asks first.

## Changes and context

- **Change ledger.** `limits.record_changes` (on): the files each command changes in its
  project are recorded (sizes, hash, a capped diff when the earlier text was read; never
  for `.env*` or `.ssh`). It feeds the chat's list of changed files. The read-only
  `changes` tool asks it by path, time and thread; off until `tools: { changes: true }`.
- **Result stubs.** `limits.stub_results` (off): old results of `run_command`,
  `read_file`, `search_files` and `read_history` are sent as a one-line stub that names the
  `read_history {"result": N}` call returning them, when that pays for the cache it breaks.
  Results of the newest `limits.stub_keep_steps` (8) steps and results under
  `limits.stub_min_tokens` (200) stay whole. The full text stays stored.
- **Tail compaction.** `limits.compact_tail` (off): a summary covers the older part; the
  newest messages, up to `limits.compact_tail_tokens` (16,000), stay word for word.
  `limits.compact_early` (off) also compacts between turns past 60% of the input limit.
- **Stable prefix.** `limits.stable_prefix` (off): each tool description stays as the
  thread first showed it until the next compaction, so memory or skill changes do not
  break the provider's cache.
- **Reading several files.** `limits.read_many_files` (off): `read_file` also takes
  `paths` (2 to 8 files in one call). `limits.parallel_reads` (on): the read-only calls of
  one step run together, at most 8.
- **Shaped command output.** `limits.shape_output` (off): the output of `cargo`,
  `pytest`, `npm`/`pnpm`/`yarn`, `tsc` and git transfers is sent without progress and
  passing tests, repeated lines folded, with a line saying how many were left out. Errors,
  warnings and `file:line` lines stay; piped or unknown commands are not shaped. The whole
  output stays stored and `read_history {"result": N}` returns it.
- **Failure ladder.** `limits.failure_ladder` (off): when the same error returns within a
  turn through calls that differ, the model is told to stop varying the call at the third
  time (`limits.failure_note_at`), asked for a different plan at the fifth
  (`failure_replan_at`), and the turn ends as failed at the eighth (`failure_stop_at`).

## Project knowledge

- **Project graph.** `limits.project_graph` (off): a background worker indexes the
  symbols and references of each saved project. The `project_graph` tool (off until
  `tools: { project_graph: true }`) answers where a name is defined, who uses it, what a
  file depends on and which tests cover it, in at most 16 rows. It is offered once the
  index is ready.
- **Episodes and `recall`.** `limits.episodes` (off): for a saved project, what ended a
  repeated failure, or a goal's verification that passed, is stored as an episode (at most
  500 a project). `limits.episode_lesson` (off) adds a one-line lesson from the model. The
  `recall` tool (off until `tools: { recall: true }`) returns up to three notes.

## What the chat shows

- **Structured blocks.** The `show` tool puts a flow, a list of checks or reply buttons in
  the chat; off until `tools: { show: true }`. See [blocks.md](blocks.md).
- **Overview.** The Statistics view of every client: the plan's progress and members,
  tokens, context, changes, tool activity and an event log. The core counts all of it;
  there is no switch.
- **Working row.** While a turn runs, the chat shows time, tokens and step, for example
  `Working · 2m 14s · 18.4k tokens · step 7`, counted by the core; no switch.

## Plan graph and teams (unfinished)

`limits.plan_graph` (off). In ORCHESTRATOR mode the lead's `tasks` tool takes a plan: tasks
that depend on each other, with files, roles and acceptance. `run` has the core give each
ready task to a member (a thread under the chat), keep a lease on it and hand it out again
if the lease runs out, and end the team when all tasks are done or a budget of steps,
tokens or minutes (`limits.team_max_steps`, `team_max_tokens`, `team_max_minutes`) is
spent. Not finished: command gates are not run (such a task waits for the lead's review)
and there is no recovery after a restart. Further switches, all off: `limits.task_packets`,
`member_worktrees`, `peer_messages`, `procedures`.
