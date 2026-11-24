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
- **Narrow re-run.** `limits.rerun_hint` (on): a failed `cargo test`, `pytest` or
  `npm test` running jest gets one line naming the command that re-runs only what failed
  (`cargo test -- --exact <names>`, the pytest node ids, `--runTestsByPath <files>`). The
  line comes only when the command line and the output leave no doubt: no filter of the
  line's own, every failure named and the names adding up to the runner's count, and
  the output not cut in the middle.
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
  file changed since, as the same command ending the same way or with the same tests
  failing, gets a note
  with the step of that run and its failing tests (`limits.test_rerun_note_at`, 2), and a
  request for a different plan at the fourth such run (`test_rerun_replan_at`). The
  same command ending another way (a service started, a flaky test passing) is news
  and starts the runs over. A narrower run to look at one failure is not a repeat, nor is the first run that names
  the one test that failed. Changes are read from the change ledger once it has
  recorded one in the chat, else any call that may write a project file counts as one
  (the chat's own goal, tasks, plan and memory, and messages, do not); a write the
  ledger cannot see (a subagent, a background job, an MCP tool, a command whose walk
  gave up on a project or change too large or slow to compare) always counts. The
  command still runs. The failing test ids also stand for a run's error in the failure
  ladder, so the same failures under another filter climb it; at its top they make
  the turn a stall candidate for the judge rather than ending it, since tests failing
  for another reason come back through real edits. Where no judge can rule they still
  stop it: at `failure_stop_at` with the judge off, at twice that once its calls are
  spent.
- **No progress.** `limits.no_progress` (on): a step makes progress when one of its
  calls changed a project file to a state not seen in the turn (by the change ledger),
  gave a result not seen in the turn (ids and times made alike), read a file, range or
  search not asked for before, or passed a command that failed earlier in the turn.
  Steps in a row without any get a note asking where the turn stands
  (`limits.no_progress_note_at`, 12), then a request for a different plan
  (`no_progress_replan_at`, 30), and again every `no_progress_note_at` steps after. Until
  the ledger records a change in the turn, and for writes it cannot see (a subagent, a
  job, an MCP tool, a command whose walk gave up), a new call that may write a project
  file counts as progress (the chat's goal, tasks, plan and memory are no project
  file); a step of only looks at background jobs, or of plain waits (`sleep 30`) once
  a job started in the turn, is not counted. Notes only: after the re-plan the turn is a stall
  candidate for a judge, and goes on.
- **Loop judge.** `limits.loop_judge` (on): a turn still without progress after the
  no-progress re-plan, a cycle asked twice for a different plan, or the same failing
  tests at the failure ladder's top, is read by a judge: the arbiter's helper model
  when the user set one (`arbiter.model`), else, or when it cannot be had, the turn's
  own model, at low effort (asked again without one when the model refuses it, as one
  without reasoning does, and without one from then on) and without tools, given the
  user's request, the last 20 calls one line each, what the rule saw and the error that came back most
  (`resources/internal/LOOP_JUDGE.md`, which says varied retries and batch edits are not
  loops). It answers `{stuck, confidence, redirect}`. Only `stuck` at 0.9 or more ends
  the turn as failed, with the last calls and the suggestion (`JudgeStop`); otherwise
  the redirect is the next step's note (`JudgeRedirect`) and the next candidate waits
  for one more mark than the last did. A cycle counts once per step toward the two
  re-plans, and a call outside it or one that brings something new starts the count
  over. At most `limits.loop_judge_calls` (3) a turn; an answer that cannot be read
  lets the turn go on. Its tokens are logged, not kept in the usage ledger.
- **Text loops.** `limits.text_loops` (on): the model's answer is read as it streams in
  50-character pieces; one that comes back `limits.text_loop_repeats` (10) times close
  together (on average within 250 characters) cuts the stream, and the step is taken
  again with a note (`Cycle`: the protocol has no rung of its own for text, so a text
  loop is told as the cycle it is). It is a loop only when the text between the
  piece's places is mostly the same too, so a list of paths or links sharing a long
  start is prose. Only places that do not overlap count, so a long hash, a dotted
  leader or a bar of one character is no loop. Code (fenced or indented), tables,
  lists, headings, quotes and rules are not read, and each such line forgets what came
  before it. After two cuts in a turn the next step streams whole, so the rule never
  ends a turn or loses its calls.
- **Goal stalls.** Two rules, one per cause. Turns of a goal that call no tool are the
  evaluator's: after three in a row no turn is started, and the goal goes on after the
  user's next message. Turns that do act but change nothing are measured here:
  `limits.goal_stall` (on, needs `record_changes`): after each round of
  a goal, the state of the files its chat and agents changed and the test results of
  the round (error lines included, so a build failing with fewer errors is another
  result) are stored with it; once `limits.goal_stall_rounds` (3) measured rounds in a row
  left both as they were, the goal pauses for the user, saying what stayed the same. A
  round that changed no file and ran no test is not measured, nor one that may have
  written where the ledger does not look (a job, a subagent, an MCP tool, a command too
  large to compare); a look at the goal or the tasks is no write. Nor is any round
  in a chat the ledger never recorded a change of. A resume starts the count over.

## Project knowledge

- **File map.** `limits.project_graph` (on): the `project_graph` tool (`tools: { project_graph: false }`
  hides it) maps the working folder by file and folder names, never their content: an
  indented tree that folds deeper folders to `name/ (N files)` to stay under 300 lines and
  12 KiB, one folder of it on `path`, or the paths matching `find` (a part of a path or a
  glob). It walks as the change ledger does (ignore rules, no `.git`, no links followed) on
  every call; nothing is indexed or stored. The name is kept from the code graph it replaced.
- **Episodes and `recall`.** `limits.episodes` (on): for a saved project, what ended a
  repeated failure, or a goal the evaluator found met, is stored as an episode (at most
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

In TEAM mode the user is not the team lead. The agent leads the project like a product
owner or team lead: it plans the work and runs a team of agents, each with its own role
and task, and owns the delivery; the user is its client, who sets the goal and answers
its questions.

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
- **No budget; ends.** A team has no step, token or time budget: what it used is shown,
  never a limit. `limits.team_at_once` is the most members at work at once; the user may
  change it while the team runs, from 1 up to that setting. The team ends when all is done, it is stopped, the lead
  went quiet at the cap of agent messages, its provider keeps failing, or nothing can
  start and the lead changed nothing. Older configs' `team_max_steps`, `team_max_tokens` and `team_max_minutes` are
  read and dropped.
- **Wind-down.** The lead's `stop`, the Team page's Stop, the quiet cap and a provider
  that keeps failing (a key or quota it refuses, or three member turns in a row lost to
  it) do not cut members off: the team gives out nothing new, and the core (not the lead)
  asks each member at work to stop and answer with its footer. A task it finished hands
  in as usual (its gate runs with the next run); one it did not goes back to open with
  the footer kept, and the next member's card starts with that hand-over. The team ends
  once nobody is at work or `limits.team_wrap_seconds` (180) ran out; clients say
  "wrapping up" meanwhile, and a second stop ends it at once. A stop of the chat or of a
  member by the user still ends the team at once. A loss that is not the member's (a
  restart, a provider failure, a turn cut off while winding down or by the team's end)
  is not counted against the task's three attempts, up to three such losses of one task;
  a lease that runs out while the team runs still counts. A provider's end reads
  "stopped" with the provider's reason in the lead's line.
- **Resume.** A task its member lost goes back to that member: the same thread, with
  its transcript, gets a short note ("Your last turn on #id stopped: … Continue from where
  you are") instead of a new card. A turn that failed on a context too long or a request
  the provider refused, a member that lost the task twice, a turn still being cut off (a
  lease that ran out), a review, the critic, and a task the lead edited or reopened go
  to a fresh member instead, whose card says how the last attempt ended. Each member
  carries a one-line summary of what it does (`TeamMember.summary`), and the lead's lines
  name the member, its role and its task.
- **Recovery.** When the chat is opened after a restart, an ended team's work goes back
  to open, a turn still running gets its lease again, a turn that completed is handed in,
  a lost one goes back to open without counting the attempt, and a gate that was cut off
  runs again.

Not built: a worktree per member, messages between members, procedures (their switches
`limits.member_worktrees`, `peer_messages`, `procedures` do nothing yet; `task_packets`
is no longer read).
