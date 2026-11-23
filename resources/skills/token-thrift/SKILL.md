---
name: token-thrift
description: "Spending fewer tokens and steps for the same result: when to think, what to read, how to batch tool calls and keep results short. Use on long or tool-heavy work."
source: "duskcode"
author: "duskcode"
license: MIT
tags: [efficiency, tokens]
---
# Token thrift

The cost of work is the tokens read and written and the steps taken. Cut what does not
change the result.

## Think only when it changes the next step

- Do not restate the task, the plan or what a tool just returned.
- Think before a choice that is costly to undo; act at once when the next step is clear.
- When unsure, a cheap check (a search, a test run) beats a long guess.

## Read slices, not whole files

- Search first (`search_files`, `rg`), then read the lines around the match.
- Read a long file in parts, only the parts you need; never read the same lines twice.
- List a folder before opening files in it; skip lockfiles, build output and vendored code.

## Batch independent tool calls

- Calls that do not depend on each other go in one step: reads of several files, a
  search and a test run, several commands.
- Chain dependent shell commands with `&&` in one call instead of one call each.
- Start a long command with `background: true` and go on; its result arrives by itself.
  Never poll with `sleep`.

## Keep results short

- Ask commands for less: `--quiet`, `| head`, `| tail`, `grep` for the line you need.
- Run one test, not the suite, while iterating; the suite once at the end.
- Report the conclusion and the evidence that proves it, not the raw output.

## Keep prefixes stable

- Providers cache the start of a conversation. Do not reorder or rewrite earlier
  context; add new facts at the end.
- Load a skill once per thread and reuse it; do not paste the same text again.

## Write less

- The smallest change that does the job; edit in place rather than rewrite a file.
- Answers: the result first, then only what the reader needs to act on it.
