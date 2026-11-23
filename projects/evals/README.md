# Evals

Benchmark tasks that say whether a change to prompts, skills, agents or tools made the
agents better. Each task is a small project with a job to do and a script that checks the
result, so a verdict is a command's exit code and not an opinion.

```bash
cargo run -q -p dusk_evals -- --selfcheck                                              # check the harness without calling a model
cargo run -q -p dusk_evals -- --runs 3 --out projects/evals/results/shipped.json       # run every task 3 times with the shipped prompt
git show v0.69.1:resources/modes/MAIN.md > old.md                                      # save the old MAIN prompt
cargo run -q -p dusk_evals -- --runs 3 --main-prompt old.md --out projects/evals/results/old.json  # run again with the old prompt
cargo run -q -p dusk_evals -- --compare projects/evals/results/old.json projects/evals/results/shipped.json  # compare the two runs
```

`--selfcheck` needs no model: each check must fail on the untouched task and pass on its
known solution. Run it after changing a task.

A run uses a temporary config folder holding a copy of `config.yml`, for the provider and
model, and the rules in this repository, so your own folder is not touched and the copy is
deleted afterwards. `--mode` and `--only` narrow a run; `--runs` repeats it, since models
are not deterministic. One run proves little: compare over several.

`--set NAME=VALUE` changes one key of that copy for the run, to measure a harness
mechanism on and off: `--set limits.max_attempts=1`, `--set memory.enabled=false`,
`--set effort=low`. It may be repeated. NAME is a dotted key under `agents`, `approvals`,
`effort`, `limits`, `memory`, `mode`, `model`, `skill_packs`, `skills`, `thread`, `tools`,
`version_control` or `work_history`; VALUE is one YAML value. The core refuses a key it
does not know, so a misspelt name stops the run before any task. In `tools`, `skills`,
`skill_packs` and `agents` the core takes any name, so only a name already in the
`--config` file can be set. Providers, MCP servers, plugins and voice cannot be set: they
hold keys, and the settings are written into the report.

Agents run with their usual tools on your machine, in a temporary project folder. That is
not a sandbox: `run_command` can reach anything you can.

A task is a folder in `tasks/`: `task.md` is the request, `fixture/` the project, `check.sh`
decides from the working folder, with the agent's last message in the file `$EVAL_ANSWER`,
`solution.sh` is a known way to pass, and `answer.txt` the message it is checked with.
`setup.sh`, if any, adds to the project. The agent never sees the check. `run.yml`, if
any, says how the runner runs the task; its one key is `kill_after`.

## What a report holds

Each run records its verdict, time, model calls and a token ledger taken from the core's
own count, subagents included: prompt tokens served from the provider's cache, not
served from it, and written to it; completion tokens and the reasoning among them. Beside
those: tool calls and errors, provider retries, and re-runs (tool calls with the name and
arguments of an earlier call of the same agent). For context work: stub moves, the results
they put away and the tokens they saved (`limits.stub_results`), command results sent
shaped and the tokens that saved (`limits.shape_output`), retrievals (`read_history`
calls that read one stored result back), and finished compactions.

The headline is the pass rate and tokens per solved task: every token of every run,
failed and timed-out ones included, over the runs that passed; `n/a` when none did. A run
that times out has its turn stopped and keeps what the core counted until then; it is
left out of the per-run means, and the summary says how many were. `--compare` prints
both, per task and overall, with each side's `--set` keys.

## Recording a baseline

Record the shipped engine first, then the same tasks with one mechanism changed, with the
same model, runs and timeout, and compare:

```bash
cargo run -q -p dusk_evals -- --runs 3 --timeout 600 --out projects/evals/results/baseline.json  # baseline run with a 600 s timeout
cargo run -q -p dusk_evals -- --runs 3 --timeout 600 --set memory.enabled=false \
    --out projects/evals/results/no-memory.json  # same run with memory switched off
cargo run -q -p dusk_evals -- --compare projects/evals/results/baseline.json projects/evals/results/no-memory.json  # compare the two runs
```

The `hard-` tasks hide what the request does not spell out: randomized checks against a
reference, concurrency under stress, encoded attacks, a specification with amendments, a
large log. `--only hard-` runs them alone.

## Loop tasks

The `loop-` tasks tempt an agent to go round in circles, to measure the repeat and
no-progress guards (`limits.failure_*`) on and off. The report counts `test_runs`
(commands that ran tests, subagents included), repeat notices and repeat stops beside
the verdict.

| Task | The trap | A pass |
|---|---|---|
| `loop-flaky-test` | a test that fails one run in three (a counter file decides) and passes when run again | the retry bug is fixed; the check runs every case |
| `loop-narrow-test` | a one-line bug in a suite of thirty two-second modules; the README names the narrow command | the fix, with the whole suite run at most twice |
| `loop-ping-pong` | two tests that seem to want opposite values of one setting | the setting stays in milliseconds and the client converts |
| `loop-missing-dependency` | a package from an index that cannot be reached | a report that it is missing, no stand-in and no claim of a pass |

## Team tasks

The `team-` tasks have parts for a lead and its members (the plan graph in
`docs/features.md`). Each part has its own files and a check command named in `task.md`, which a plan can
take as the part's gate; `check.sh` runs those and hidden cases of each part. They run as
well for one agent, the baseline they are compared with: `--mode TEAM` for a team,
the default mode for one agent.

| Task | Design | What it shows |
|---|---|---|
| `team-fan-out` | Q1 | four modules of `textkit`, each with its tests, no file shared |
| `team-chain` | Q2 | schema, then API, then command line of an SQLite stock book; each needs the one before |
| `team-chain-kill` | Q5 | `team-chain`, linked, with the core killed once a run and started again |
| `team-small` | Q7 | three short tasks (`add-cli-flag`, `fix-pagination`, `thread-a-setting`) in one project, to see what a plan costs when none is needed |

`kill_after: 2..=40` in a task's `run.yml` kills the core once in each run, after that many
tool events (a call's start or end, of the chat or any agent), picked from the range by
`--seed` (default 1), the task and the run's number: the same seed gives the same points,
and `--runs 20` gives twenty points over the range. `kill_after: 12` is one point. The core
runs on a tokio runtime of its own; a kill drops that runtime, so every task of the core
stops where it stands and no shutdown code runs, then a new core starts on the same folders
and the runner opens the thread again, as a person would after a crash. Unlike a killed
process, what the core held is dropped, so a command `run_command` started is killed with
it (`kill_on_drop`) where a crash would leave it running; blocking work still running after
10 s outlives the kill.
The report has `killed_after` per run; a run that ended before its point has none.

```bash
cargo run -q -p dusk_evals -- --only team- --mode TEAM --runs 4 --out projects/evals/results/team.json  # run the team- tasks in TEAM mode, 4 times each
cargo run -q -p dusk_evals -- --only team-chain-kill --runs 20 --seed 1 --out projects/evals/results/kill.json  # repeat the kill task 20 times, fixed seed
```

## What is known so far

On qwen3.8-flash, three runs of each task, MAIN of v0.69.1 against v0.69.4: both passed
36 of 36, the new one at 14% more tokens and 27% more time (means). These tasks are too
easy for this model to tell the two prompts apart, so they show the agents do not break
and say nothing yet about which prompt is better. Harder tasks are what is missing.
