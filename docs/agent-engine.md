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
- [x] Compaction sends the thread's tools, the project text and the thread's cache key, so
      its request starts with the prefix the steps cached (it was read cold: 577k prompt
      tokens in 3 calls, 0% cached); a model that calls a tool instead is asked again
      without tools.
- [x] Subagents: phases are optional; the runtime starts the task in planning and marks
      it done on the answer, so a subagent does not spend steps on bookkeeping.
- [x] Think once, then act: the first step of a turn and a step after a failed tool
      run at the effort set, steps that carry out the plan at a light one (low at most,
      only when a level is set); the prompts ask for one plan, then tool calls, and
      reasoning only on a surprise. Measured on qwen3.8-flash (6 eval tasks, 2 runs each,
      effort medium, before vs after): reasoning tokens 2647 to 1681 (-36%), output
      6613 to 4624 (-30%), all tokens 186k to 141k (-24%), calls 14.7 to 12.1, tool
      errors 1.9 to 0.8; passed 9/12 to 8/12, the one lost run a 600 s timeout on
      hard-log-forensics. Too few runs to call the pass rate; the token drop is clear.
- [ ] Compaction prepared before the limit, recent tail kept word for word.
- [ ] An eval set (a Terminal-Bench subset through Harbor) to measure each change.

## Cache audit (2026-11-10, 5,600 answer calls on qwen3.8-flash)

Read from `model_usage`; no per-call cache line existed in `core.log`, so this was
reconstructed from the record. Since then each call logs `model call` with prompt, cached,
written and completion tokens, and a `prompt cache dropped` warning when less than half of
the last prompt was read from the cache.

| Finding | Number | Reading |
|---|---|---|
| Prompt tokens read from cache | 93.3% (370M of 396M) | the prefix is stable; steady state is fine |
| Median uncached tokens in a later step of a turn | 763, new result 843 | only the new tool result is read cold |
| Misses over 5k tokens | 84% of all uncached tokens, 1,079 calls | losses are events, not drift |
| Calls with zero cache and a prompt over 20k | 132 calls, 9.9M tokens (37% of misses) | clustered while two threads ran at once: one server cache slot, evicted by the other thread |
| Misses after a gap over 5 minutes | 24% of misses | the cache aged out; unavoidable on a local server, a TTL on hosted ones |
| Compaction calls | 3 calls, 577k prompt tokens, 0% cached | fixed: they now share the steps' prefix |
| Share of stored content that is tool output | 86% (2.55M of 2.97M tokens) | each result is re-read on every later call, about 130 times over a long run |
| Tool output by tool | `run_command` 1.61M, `read_file` 0.67M, `search_files` 0.11M | |

What is left, by expected gain:

- Interleaved threads on one local server. Fix is outside the core: run the server with
  one slot per concurrent agent (`--parallel`), or lower the agent limit while a goal runs.
  A per-thread slot hint (`id_slot`) would pin it, but only llama.cpp takes it, and other
  providers reject unknown fields, so it needs a per-provider switch first.
- Carry cost of old tool results: replace results older than N steps and larger than M
  tokens with a one-line stub (what ran, its size, how to run it again). One cache break,
  then a smaller prefix for the rest of the run. Not done: it needs the eval set to prove
  the model does not re-run commands it no longer sees.
- A longer cache lifetime on hosted providers (Anthropic one-hour TTL) for threads that
  pause between turns; costs twice the write price, so only for threads that come back.

## Rules in tool descriptions (2026-11-10)

The rules about running commands, reading, asking agents, asking the user and the task
board moved from the MAIN prompt into the description of each tool, so they sit beside the
choice they guide and appear only when the tool is on. MAIN went from 3,915 to 2,473 bytes;
the descriptions grew by about 800 characters, so a request is about 160 tokens shorter.

Measured on qwen3.8-flash with `projects/evals`, old code and prompt against new:

| Set | Passed | Calls | Tokens | Tool errors |
|---|---|---|---|---|
| 6 tasks, 2 runs each | 7/12 and 6/12 | 4.8 and 3.5 | 94k and 69k | 0.6 and 0.2 |
| 2 disputed tasks, 4 runs each | 4/8 and 4/8 | 4.0 and 4.1 | 77k and 80k | 0.0 and 0.1 |

The 27% drop in the first set is not reliable: a run that times out counts as zero calls
and zero tokens, and the new version had two such runs (`hard-ttl-cache`, which takes about
as long as the 400 s limit with either prompt) against one. On the two tasks rerun with four
runs each, the pass rate and the cost are the same. A run that answered with no tool at all
happened with the old prompt as well. Reading: no measurable change in quality or in work
done; the saving is the shorter prefix on every request. Too few runs to see a small
difference either way.
