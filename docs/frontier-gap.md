# What separates duskcode from the frontier agents, by efficiency

Compared with Claude Code, Codex CLI, opencode and Hermes Agent. Rules and limits are not
the question; the question is what gets the same task done in less time and fewer tokens.
Each line says where it comes from: **measured** (duskcode's own record, see
`agent-engine.md`), **sources** (what their docs or write-ups say, found 2026-11-10; the
benchmark tables in those write-ups are vendor or blog figures, not evidence), or
**check** (my recollection of the tools, not re-verified: read their docs before relying
on it).

## What the others have that decides speed

| Gap | What they do | duskcode now | Evidence |
|---|---|---|---|
| Measuring every change | Teams run a benchmark per change (Terminal-Bench, SWE-style sets) and trade pass rate against cost | 12 small eval tasks, 2 runs each; a pass rate cannot be read from that | measured |
| Headless mode | `claude -p`, `codex exec`, `opencode run`: one prompt in, result out; the way benchmarks, CI and scripts drive an agent | `duskcode run "prompt"` (done); evals still drive the core directly | measured |
| Old tool output | opencode prunes old tool outputs; Claude Code can clear stale tool results | every result stays until compaction; 86% of stored content is tool output, re-read about 130 times in a long run | measured / check |
| Lazy tool loading | Claude Code loads MCP tool definitions on demand when they grow large | all enabled tools go in every request; the 128-tool cap is gone, so a big MCP server now costs every call | check |
| Feedback after an edit | opencode feeds language-server diagnostics back; the shell tools of all of them show test output | the model learns of a type error only if it runs the build; 7% of edits failed on "found 0 matches" before the edit tools were removed | measured / check |
| Edit format the model was trained on | Codex `apply_patch`, Claude Code string replace | edits go through the shell (heredocs, sed); cheap on average, untested against a patch tool on the local model | check |
| Cheap model for side work | Claude Code runs exploration and summaries on a small model | one model for steps, compaction, titles and subagents unless an agent sets its own | check |
| Parallel work | several tool calls per step, subagents in parallel | 83% of steps make one call; the prompt asks for batches, no more | measured |
| Compaction before the limit | opencode prepares it in the background and keeps the recent tail verbatim | compacts at the limit, cold until now; prefix reuse is in, background compaction is not | measured |
| Learning from practice | Hermes writes a skill after a hard task and refines it while it is used | skills `skill-from-practice` and `skill-improve` exist, run by the user | sources |

## Where duskcode is already level or ahead

- Cache hit rate is 93% on the local model; the losses are events (two threads on one
  server slot, a pause over five minutes), not drift. [measured]
- One core, two clients on one protocol, a confining sandbox on macOS and Linux, a goal
  loop with verification, background jobs, and per-thread approval modes. [measured, code]
- Think once, then act: reasoning tokens -36% at the same effort setting. [measured]

## What to do, by expected gain

1. **An eval run that can be trusted.** A Terminal-Bench subset through Harbor, with
   pass rate, tokens, time and cost per task, run before and after each change below.
   Without it every item here is a guess.
2. ~~A headless mode~~ done: `duskcode run`, with `--json` usage. The eval runner can use
   it to run a task the way a user would.
3. **Stub old tool results.** Replace a result older than N steps and larger than M
   tokens with one line (what ran, size, how to see it again). One cache break, then a
   smaller prefix for the rest of the run. Prove with 1 that the model does not re-run
   what it no longer sees.
4. **Load MCP tools on demand**: send names and one-line descriptions, load a tool's schema
   when the model asks for it.
5. **A cheap model for compaction, titles and read-only subagents**, a setting per role.
6. **Diagnostics after edits**: run the project's check command named in its config after
   a step that changed files, and show only new errors.
7. **A patch tool against the shell, as an experiment** on the eval set, kept only if it
   wins on tokens and failures for the local model.
8. **Background compaction** with the recent tail kept word for word.
