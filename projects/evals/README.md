# Evals

Benchmark tasks that say whether a change to prompts, skills, agents or tools made the
agents better. Each task is a small project with a job to do and a script that checks the
result, so a verdict is a command's exit code and not an opinion.

```bash
cargo run -q -p dusk_evals -- --selfcheck
cargo run -q -p dusk_evals -- --runs 3 --out projects/evals/results/shipped.json
git show v0.69.1:resources/modes/MAIN.md > old.md
cargo run -q -p dusk_evals -- --runs 3 --main-prompt old.md --out projects/evals/results/old.json
cargo run -q -p dusk_evals -- --compare projects/evals/results/old.json projects/evals/results/shipped.json
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
`setup.sh`, if any, adds to the project. The agent never sees the check.

## What a report holds

Each run records its verdict, time, model calls and a token ledger taken from the core's
own count, subagents included: prompt tokens served from the provider's cache, not
served from it, and written to it; completion tokens and the reasoning among them. Beside
those: tool calls and errors, provider retries, repeat notices (results the core marked
as the same call giving the same result a third time in a row), and re-runs (tool calls
with the name and arguments of an earlier call of the same agent).

The headline is the pass rate and tokens per solved task: every token of every run,
failed and timed-out ones included, over the runs that passed; `n/a` when none did. A run
that times out has its turn stopped and keeps what the core counted until then; it is
left out of the per-run means, and the summary says how many were. `--compare` prints
both, per task and overall, with each side's `--set` keys.

## Recording a baseline

Record the shipped engine first, then the same tasks with one mechanism changed, with the
same model, runs and timeout, and compare:

```bash
cargo run -q -p dusk_evals -- --runs 3 --timeout 600 --out projects/evals/results/baseline.json
cargo run -q -p dusk_evals -- --runs 3 --timeout 600 --set memory.enabled=false \
    --out projects/evals/results/no-memory.json
cargo run -q -p dusk_evals -- --compare projects/evals/results/baseline.json projects/evals/results/no-memory.json
```

The `hard-` tasks hide what the request does not spell out: randomized checks against a
reference, concurrency under stress, encoded attacks, a specification with amendments, a
large log. `--only hard-` runs them alone.

## What is known so far

On qwen3.8-flash, three runs of each task, MAIN of v0.69.1 against v0.69.4: both passed
36 of 36, the new one at 14% more tokens and 27% more time (means). These tasks are too
easy for this model to tell the two prompts apart, so they show the agents do not break
and say nothing yet about which prompt is better. Harder tasks are what is missing.
