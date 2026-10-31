# Evals

Benchmark tasks that say whether a change to prompts, skills, agents or tools made the
agents better. Each task is a small project with a job to do and a script that checks the
result, so a verdict is a command's exit code and not an opinion.

```bash
cargo run -q -p dusk_evals -- --selfcheck
cargo run -q -p dusk_evals -- --runs 3 --out evals/results/shipped.json
git show v0.69.1:resources/modes/MAIN.md > old.md
cargo run -q -p dusk_evals -- --runs 3 --main-prompt old.md --out evals/results/old.json
cargo run -q -p dusk_evals -- --compare evals/results/old.json evals/results/shipped.json
```

`--selfcheck` needs no model: each check must fail on the untouched task and pass on its
known solution. Run it after changing a task.

A run uses a temporary config folder holding a copy of `config.yml`, for the provider and
model, and the rules in this repository, so your own folder is not touched and the copy is
deleted afterwards. `--mode` and `--only` narrow a run; `--runs` repeats it, since models
are not deterministic. One run proves little: compare over several.

Agents run with their usual tools on your machine, in a temporary project folder. That is
not a sandbox: `run_command` can reach anything you can.

A task is a folder in `tasks/`: `task.md` is the request, `fixture/` the project, `check.sh`
decides from the working folder, with the agent's last message in the file `$EVAL_ANSWER`,
`solution.sh` is a known way to pass, and `answer.txt` the message it is checked with.
`setup.sh`, if any, adds to the project. The agent never sees the check.
