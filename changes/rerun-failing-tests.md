Added
A failed `cargo test`, `pytest` or `npm test` running jest is followed by one line naming the command that re-runs only the failing tests, when the command line and the output leave no doubt about it (`limits.rerun_hint`, on). The main and agent prompts ask to iterate with the tests that cover a change and run the full suite once before calling the work done.
