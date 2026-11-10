# Security policy

duskcode runs commands and reads and writes files for a model, so a flaw in how it confines
them, keeps secrets or reads untrusted input is a security flaw, not just a bug.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub's
[private vulnerability reporting](https://github.com/DroBov1Tya/dusk_code/security/advisories/new)
for this repository, with what you did, what happened and the version (`duskcode --version`).

You can expect an answer within a week. A confirmed flaw is fixed in the next release, and
credited to you if you wish.

## What is in scope

- Escaping the sandbox or the folders a chat is confined to; running a command the approval
  mode should have stopped.
- Secrets (API keys, project variables, MCP tokens) leaking to logs, the model, a client or
  a file.
- Content from a file, page or tool result being obeyed as an instruction.
- The updater or the installer fetching or running something it should not.

## Supported versions

Only the latest release, and the latest pre-release on the dev channel.
