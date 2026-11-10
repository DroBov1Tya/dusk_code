# Closing the gap for weak models

What makes an agent like Claude Code work well with a model that is not the strongest is
mostly the harness, not the model. This is read from how the harness around the author of
this note behaves, and from duskcode's own record (`agent-engine.md`). Each item says
what duskcode has, what it lacks, and how it would be proved on the eval set.

## What a strong harness does, and where duskcode stands

| Practice | In the strong harness | duskcode |
|---|---|---|
| Rules sit beside the decision | The rules for a tool are in the tool's own description ("read the file before editing", "prefer this tool over the shell"), not only in the system prompt; a weak model obeys what it reads next to the choice | System prompt carries them; tool descriptions are short. Move each tool's rules into its description |
| Edit that teaches | An edit that misses says why (not found, or found twice) and the file must have been read first | Edit tools were removed; edits go through the shell, where a miss is silent. Measure a patch tool against heredocs on the evals |
| Lossless big output | A long result is kept in full in a file and the model gets a head, a tail and the path | The shell keeps 8 KiB head and 24 KiB tail and drops the middle. Spill the whole stream to a file in the chat folder and name it |
| Reminders at the right time | Short reminders are injected as the work goes: the task list when it is stale, the date, the git state | The plan and task tools exist; nothing nudges a model that forgot its list |
| Stuck detection | A repeated failing call gets a push to change course | Added: from the third identical call with the same result, the result carries a note. It replaces the old 150-step cap |
| Small, typed subagents | Each kind of subagent has a fixed tool set and a fixed way to report; read-only ones run on a cheap model | Agents are defined in Markdown with their own tools; no cheap-model setting per role yet |
| Tools on demand | Only names of rarely used tools are in the prefix; a schema is fetched when needed | All enabled tools are in every request (see `frontier-gap.md`) |
| Verification built in | After a change the harness can run the project's checks and show only what is new | Goal verification exists for goals; no per-step check after edits |
| Plain, actionable errors | An error says what to do next | Mostly, and each tool's error text should be reviewed with the same test |

## What a weak model gets wrong, in the record

- Edits text it has not read exactly (7% of edits missed before the edit tools went).
- Polls with `sleep` (50 times) instead of waiting for a background job.
- Starts most commands with `cd` (74%).
- Calls one tool per step in 83% of steps, so every step re-reads the context.
- Thinks for 80% of its output tokens; the plan-then-act rule cut that by a third.
- Repeats a failing call; this is what the new note is for.

## Order of work

1. The eval run that can be trusted (`frontier-gap.md`), since each item below is a claim.
2. Rules into tool descriptions, one tool at a time, each run on the evals.
3. Spill long output to a file instead of cutting its middle.
4. Reminders: stale task list, a long run with no plan.
5. A check command after edits, showing only new errors.
6. A cheap model for compaction, titles and read-only subagents.
