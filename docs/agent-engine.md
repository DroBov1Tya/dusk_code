# Agent engine: where the tokens and the time go

Measured on 2026-11-10 from the local record (`model_usage`, `messages`): 5,508 model
calls in 352 turns, almost all on qwen3.8-flash.

## What the record shows

| Measure | Value | Reading |
|---|---|---|
| Model calls per turn | 15.6 on average, 150 at most | every call re-reads the whole context |
| Output that is reasoning | 80% (6.0M of 7.5M tokens) | the model thinks far more than it writes |
| Prompt tokens served from cache | 93.6% on qwen, 0% on deepseek | the prefix is stable; deepseek's cache is not counted or not hit |
| First call of a thread | 4.7k to 43k prompt tokens, 10k on average | fixed cost: system, tools, memory, project files |
| Steps with one tool call | 83% (4,282 of 5,201) | independent work is rarely batched |
| `run_command` share of calls | 68% (4,159) | the model works through the shell |
| `edit_file` failures | 7% (49 of 674), all "found 0 matches" | the model edits text it has not read exactly |
| `sleep` commands | 50 | polling instead of background jobs |
| Tool result size | 413 tokens on average, 16k at most | fine on average; the tail is the risk |

## Why opencode 2 got cheaper and better

opencode 2 went from 43% to 66% solved on GLM-5.3-Flash and from $3.35 to $0.45 a task
(the user's figures from a Harbor run; part of the old gap was setup). Its changes that
fit duskcode, from its source at v2.2.6:

1. A short system prompt; tool advice only for tools that are on.
2. Compaction prepared in the background before the limit, with the recent tail kept
   word for word and graded shrinking when the provider says "too long".
3. Retries with exponential backoff and jitter, `retry-after` honored, a separate time
   budget for unreachable hosts, never a retry once the answer has started.
4. Tool input repair (numbers as strings, JSON as a string) and an edit miss that shows
   the nearest lines, so the model fixes the edit without reading again.
5. Output caps: 2000 lines or 50 KiB per read; the shell keeps the tail, where errors are.
6. Prompt cache care: nothing changes the system prompt mid-thread; a cache key per thread.

The evidence on tool design (Codex, Terminus, Vercel d0): models do best with
the interface they were trained on. Fewer, generic tools win when the generic tool is
the shell. So duskcode keeps a read tool with limits and line windows, and every change
to files goes through `run_command`, confined by the sandbox like any other command.
Each tool less is a shorter prefix on every request and one fewer choice per step.

## Plan, by gain

- [x] `read_file` reads a window: 2000 lines or 50 KiB from an offset.
- [x] `run_command` keeps the first 8 KiB and the last 24 KiB of each stream.
- [x] Write, edit and delete tools removed; the shell does it inside the sandbox.
- [x] The shell keeps its folder between calls (74% of commands, 3,078 of 4,159, began with `cd`), takes
      the user's PATH, and a call says what it does in a few words for the user.
- [x] Prompt: batch independent calls in one step; think only about what decides the next
      step; no `sleep` polling, use background commands.
- [ ] Run read-only tool calls of one step in parallel. Low gain: those tools take
      milliseconds, and the long ones (commands) change things; batching per step is
      what saves time, and the prompt asks for it.
- [x] Retries: six attempts, backoff with jitter, `retry-after`, no retry after the answer
      started; stream errors without a clear cause count as transient.
- [x] Client-side token bucket per provider, so a goal loop cannot get the key banned.
- [x] Cache: deepseek's cache hits counted; `prompt_cache_key` per thread for OpenAI;
      a mode switch changes the prefix once, at the next turn.
- [ ] Compaction sends the thread's tools too, so its request reuses the cached prefix.
- [x] Subagents: phases are optional; the runtime starts the task in planning and marks
      it done on the answer, so a subagent does not spend steps on bookkeeping.
- [ ] Compaction prepared before the limit, recent tail kept word for word.
- [ ] An eval set (a Terminal-Bench subset through Harbor) to measure each change.
