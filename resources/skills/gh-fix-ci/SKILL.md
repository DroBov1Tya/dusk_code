---
name: "gh-fix-ci"
description: "Use when a user asks to debug or fix failing GitHub PR checks that run in GitHub Actions; use `gh` to inspect checks and logs, summarize failure context, draft a fix plan, and implement only after explicit approval. Treat external providers (for example Buildkite) as out of scope and report only the details URL."
source: "OpenAI"
author: "OpenAI"
license: Apache-2.0
upstream: https://github.com/openai/skills/tree/main/skills/.curated/gh-fix-ci
changes: "dusk_code: Markdown-only edition; removed inspection helper, use gh directly"
---


# Gh Pr Checks Plan Fix

## Overview

Use gh to locate failing PR checks, fetch GitHub Actions logs for actionable failures, summarize the failure snippet, then propose a fix plan and implement after explicit approval.
- If a plan-oriented skill (for example `create-plan`) is available, use it; otherwise draft a concise plan inline and request approval before implementing.

Prereq: authenticate with the standard GitHub CLI once (for example, run `gh auth login`), then confirm with `gh auth status` (repo + workflow scopes are typically required).

## Inputs

- `repo`: path inside the repo (default `.`)
- `pr`: PR number or URL (optional; defaults to current branch PR)
- `gh` authentication for the repo host

## Workflow

1. Run `gh auth status`. Report missing authentication without collecting secrets.
2. Resolve the PR with `gh pr view --json number,url` or the user's PR identifier.
3. Read failing checks with `gh pr checks <pr>`. Consult installed `gh ... --help`
   for supported JSON fields before requesting machine-readable output.
4. For GitHub Actions run links, use `gh run view <run-id>` and
   `gh run view <run-id> --log-failed`. If logs are unavailable, report that limit.
   For external providers, return their details URL without claiming to inspect them.
5. Summarize the failure, affected workflow and proposed correction; implement within
   the user's authorized scope. Do not publish or rerun external workflows without
   the required authorization.
6. Run targeted local checks, then inspect current PR checks when applicable. Local
   checks do not establish that GitHub Actions passed on the changed revision.

No inspection script is bundled. Keep downloaded logs outside the skill directory
and include only the relevant redacted failure excerpt in the report.
