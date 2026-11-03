---
name: systematic-debugging
description: Find the root cause of a bug, failing test, crash or wrong output before changing code. Use when something behaves unexpectedly, a fix did not hold, or the cause is not yet proven.
source: "dusk_code"
author: "Neil Ermak"
license: MIT
---

# Systematic Debugging

A fix is only as good as the explanation behind it. Change code after the cause is
shown, not while it is guessed.

## 1. Pin the symptom

- State expected and actual behaviour in one sentence each.
- Collect the exact error, stack trace, log lines and inputs. Read them in full; the
  first error usually matters more than the last.
- Reproduce it with one command through `run_command`. A bug that cannot be reproduced
  is reported as such, with what was tried, rather than patched blind.
- Note what changed recently: `git log`, `git diff`, dependency or config updates.

## 2. Narrow the place

- Shrink the reproduction: smaller input, one test, one request.
- Find the boundary where good data becomes bad. Log or print values at each step of
  the path, or read them in a debugger, until the first wrong value is found.
- Compare with a working case: an older commit, a similar code path, a passing test.
  `git bisect` finds the change that broke it when a good commit is known.
- Search the code with `rg` for the function, the error text and the other callers
  before assuming the code that failed is the code at fault.

## 3. Prove the cause

Write the hypothesis as "X happens because Y, so if Z then W". Test it with the
smallest change that would confirm or refute it: an assertion, a log line, a modified
input. Refuted hypotheses are progress; record them so they are not retried.

Stop and reconsider when:

- three fixes in a row did not hold: the model of the system is wrong, so go back to
  step 2;
- the explanation needs a coincidence or "it should not be possible";
- the bug moves when observed: suspect timing, shared state or uninitialized data.

## 4. Fix and guard

- Write a test that fails for the proven cause first, then make the fix that turns it
  green. See the `test-driven-development` skill for the loop.
- Fix the cause, not the symptom: no retry, sleep or broad catch that hides it, unless
  the cause is outside the code and that is stated.
- Look for the same mistake elsewhere with `rg`.
- Run the full relevant suite, not only the new test, and remove temporary logging.

## Report

Say what the cause was and how it was proven, what changed, which test now guards it,
and what remains uncertain.
