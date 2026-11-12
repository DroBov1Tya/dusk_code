# Contributing

Thanks for looking. The short version: small, readable changes that do one thing, checked by
running them.

## Build and check

```sh
cargo +1.99.1 build --workspace
cargo +1.99.1 fmt --all
cargo +1.99.1 clippy --workspace --all-targets   # must be clean
cargo +1.99.1 test --workspace
```

The GUI needs a newer Rust than 1.95, hence the pinned toolchain. Say in the pull request
what you ran and what it showed, and what you did not check.

## How the code is laid out

One core (`projects/core`) holds all logic and state; the terminal and the window
(`projects/tui`, `projects/gui`) draw and call it through one protocol (`projects/protocol`),
and never reach into the core directly. A new feature is a request and a response in the
protocol first, and both clients get it. Prompts, agents and skills shipped with the app live
in `resources/`.

## Changes

- The smallest change that does the job. No speculative fields, layers or generics.
- Errors are handled, never swallowed; no `unwrap` or `panic` on untrusted input.
- Comments say why, not what, in English.
- Only critical tests: security boundaries, parsing of untrusted input, crash durability and
  the thread loop. A test is never weakened to make it pass.
- One branch per feature, named for its topic; commit subjects read `area: imperative
  summary`, and the body says why. Add the change to "Unreleased" in `CHANGELOG.md`.

## Benchmarks

`evals/` holds small tasks with a script that checks the result, run by `projects/evals`.
Changes to prompts, tools or the agent loop should show their effect there; see
`evals/README.md`.

## Signing the desktop app

Releases of `Duskcode.app` are signed by one self-signed certificate, so macOS treats each
version as the same program and keeps the permissions (microphone, screen, files) and the
keychain access the user gave before. It is made once with
`sh scripts/make-signing-identity.sh <folder outside the repository>`, and the release
workflow reads it from the repository secrets `MACOS_SIGN_P12` and `MACOS_SIGN_PASSWORD`.
Without them the app is signed ad hoc and every version asks again. The certificate is not
a Developer ID and the app is not notarized, so the first start still asks once in
Privacy & Security. Losing the certificate means one more round of questions on the
version signed by its replacement; leaking it lets anyone sign as duskcode.
