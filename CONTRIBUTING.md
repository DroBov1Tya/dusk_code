# Contributing

Thanks for looking. The short version: small, readable changes that do one thing, checked by
running them.

## Build and check

```sh
cargo +1.99.1 build --workspace
cargo +1.99.1 fmt --all
cargo +1.99.1 clippy --workspace --all-targets   # must be clean
cargo +1.99.1 test --workspace
```

The GUI needs a newer Rust than 1.95, hence the pinned toolchain. Say in the pull request
what you ran and what it showed, and what you did not check.

## Your first change

1. Fork the repository and make a branch named for the topic.
2. Build and check with the commands above: `cargo +1.99.1 build --workspace`, then
   `fmt`, `clippy` and `test`.
3. Add `changes/<topic>.md`: the first line is `Added`, `Changed`, `Fixed`, `Removed` or
   `Security`, the rest is the entry. Run `python3 scripts/changelog.py check`.
4. Open a pull request to `main` and fill in the template.
5. The `ci-ok` check must pass before the change can merge.

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Writing a skill

A skill is one folder with a `SKILL.md` under `resources/skills/<name>/` (or
`~/.config/duskcode/rules/skills/<name>/` for your own). The file starts with front matter,
then plain Markdown that tells the agent when and how to do the work:

```markdown
---
name: spike
description: One sentence on what it does. Use when ... (this is how the agent decides to load it).
source: "dusk_code"
author: "Your Name"
license: MIT
---
```

`name` is the folder name. Keep the body short and in the imperative.
[resources/skills/spike/SKILL.md](resources/skills/spike/SKILL.md) is a small example.

## How the code is laid out

One core (`projects/core`) holds all logic and state; the terminal and the window
(`projects/clients/terminal`, `projects/clients/desktop`) draw and call it through one
protocol (`projects/protocol`),
and never reach into the core directly. A new feature is a request and a response in the
protocol first, and both clients get it. Prompts, agents and skills shipped with the app live
in `resources/`.

## Changes

- The smallest change that does the job. No speculative fields, layers or generics.
- Errors are handled, never swallowed; no `unwrap` or `panic` on untrusted input.
- Comments say why, not what, in English.
- Only critical tests: security boundaries, parsing of untrusted input, crash durability and
  the thread loop. A test is never weakened to make it pass.
- Commit subjects read `area: imperative summary`, and the body says why. Add the change as
  a fragment in `changes/` (see below), never as an edit of `CHANGELOG.md`.

## Branches and releases

- `main` is the one trunk. It is guarded by the `ci` workflow and a GitHub merge queue: a
  change enters through a pull request that passes the `ci-ok` check, and the queue tests
  the merged result again.
- A change lives on a short branch named for its topic, merged soon. Agents work in their
  own git worktrees. Unfinished work lands in `main` behind a switch that is off, not on a
  long-lived branch.
- Each change adds a fragment `changes/<topic>.md`: the first line is the section (`Added`,
  `Changed`, `Fixed`, `Removed`, `Security`), the rest is the entry. Parallel branches then
  never conflict in `CHANGELOG.md`; `scripts/changelog.py release X.Y.Z` folds them in.
- Nightly pre-releases are tags `vX.Y.Z` on `main`.
- A stable release is cut from a branch `release/X.Y`. A fix lands in `main` first and is
  then taken over with `git cherry-pick -x <commit>`, so the branch records where it came
  from.
- The release workflow decides the kind from the tagged commit: contained in a remote
  `release/*` branch, it is a full release (`latest`); any other tag is a pre-release.

## Benchmarks

`projects/evals/tasks/` holds small tasks with a script that checks the result, run by
`projects/evals`.
Changes to prompts, tools or the agent loop should show their effect there; see
`evals/README.md`.

## Signing the desktop app

Releases of `Duskcode.app` are signed by one self-signed certificate, so macOS treats each
version as the same program and keeps the permissions (microphone, screen, files) and the
keychain access the user gave before. It is made once with
`sh scripts/make-signing-identity.sh <folder outside the repository>`, and the release
workflow reads it from the repository secrets `MACOS_SIGN_P12` and `MACOS_SIGN_PASSWORD`.
Without them the app is signed ad hoc and every version asks again. The certificate is not
a Developer ID and the app is not notarized, so the first start still asks once in
Privacy & Security. Losing the certificate means one more round of questions on the
version signed by its replacement; leaking it lets anyone sign as duskcode.
