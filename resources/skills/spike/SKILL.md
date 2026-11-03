---
name: spike
description: Answer one open technical question with a small disposable experiment before real work starts. Use when feasibility, a library's behaviour, performance or a choice between approaches cannot be settled by reading docs or code.
source: "dusk_code"
author: "Neil Ermak"
license: MIT
---

# Spike

A spike buys knowledge, not code. It ends with an answer the user can act on and a
folder that can be deleted without loss.

## Decide that a spike is needed

Read first. If the docs, the source or the existing tests answer the question, report
that and stop. Run a spike only when the answer depends on behaviour that has to be
observed: an API under real input, a crate on this platform, a latency budget, two
designs compared on the same data.

Production work is not a spike. When the user wants the feature itself, plan it with
`tasks` and build it properly.

## Frame the question

Write down, before any code:

- **Question**: one sentence that can be answered yes, no or with a number.
- **Pass criterion**: what result settles it, for example "p95 under 20 ms for
  10k rows" or "the parser accepts every file in `fixtures/`".
- **Budget**: how long the spike may take. Stop at the budget and report what is known.
- **Out of scope**: error handling, configuration, style, tests beyond the check itself.

Show this frame to the user when the question came from them loosely; a sharp question
halves the work.

## Build the smallest thing that answers it

- Work in a throwaway folder in the thread's workspace, or a scratch branch, never in
  the code that ships.
- Hard-code inputs. Print results. One file is usually enough.
- Use real data or a faithful sample: a spike on toy input answers a toy question.
- To compare approaches, run them on the same input, in the same process or harness,
  several times, and keep the raw numbers.
- Run it with `run_command` and read the actual output; never infer a result from code
  that was not executed.

Independent variants can run as separate agents with `ask_agent`, each with the frame
and one variant, when they would otherwise take long one after another.

## Report

Answer in this order:

1. The answer to the question, and whether the pass criterion was met.
2. The evidence: the command, the input and the measured output.
3. What the spike did not cover and could still change the answer.
4. A recommendation for the real implementation, including what to keep from the spike
   (an API call sequence, a configuration value) and what to discard.

Then remove the throwaway folder or say where it is, so nobody mistakes it for working
code.
