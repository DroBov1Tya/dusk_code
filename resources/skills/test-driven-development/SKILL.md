---
name: test-driven-development
description: Build a feature or fix a bug test first, in short red, green, refactor cycles. Use when adding behaviour, fixing a bug with a regression test, or changing code that already has tests.
source: "dusk_code"
author: "Neil Ermak"
license: MIT
---

# Test-Driven Development

The test states the behaviour before the code exists, and the failing run proves the
test can catch its absence. Each cycle is small enough to finish in minutes.

## Before the first cycle

- Find how the project tests: the framework, where tests live, the command that runs
  one test and the one that runs them all. Read a few existing tests and follow them.
- Split the work into behaviours, one sentence each, and keep them in `tasks`. Start
  with the simplest case that is still meaningful.

## The cycle

1. **Red.** Write one test for the next behaviour. Name it after the behaviour, not the
   function. Run it with `run_command` and watch it fail for the expected reason: a
   wrong value or a missing function, not a typo or a broken import. A test that
   passes at once tests nothing new; change it or delete it.
2. **Green.** Write the least code that passes it. Hard-coding is acceptable while
   only one example forces the result; the next test will force the general rule.
   Run the test, then the surrounding suite.
3. **Refactor.** With everything green, remove duplication and clarify names in both
   code and tests. Run the suite after each change. Never refactor on red.

Commit or checkpoint at green when the project's workflow allows it.

## What to test

- Behaviour through public interfaces, not private details, so refactoring does not
  break tests.
- Boundaries: empty input, one element, the maximum, invalid input, errors from
  dependencies.
- For a bug: first a test that reproduces it and fails, then the fix. The
  `systematic-debugging` skill covers finding the cause.
- For parsers and untrusted input, add property-based or fuzz tests where the project
  has the tooling.

Replace slow or external dependencies (network, clock, randomness) at a seam the code
already has or that is worth having. Do not mock the unit under test.

## When the cycle stalls

- A test is hard to write: the design is telling you something; add a seam or split
  the unit before forcing the test.
- Green needs a large change: the step was too big; revert to the last green and write
  a smaller test.
- Existing code has no tests: first add characterization tests that pin down current
  behaviour, then change it.

## Done

All behaviours in `tasks` have tests, the full suite passes, no test was skipped or
weakened to get there, and formatter and linter are clean. Report the tests added and
the command that runs them.
