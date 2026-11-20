# Changelog fragments

Each change adds one file here, `<topic>.md`, so parallel branches never edit the same
lines of `CHANGELOG.md`. The first line is the section (`Added`, `Changed`, `Fixed`,
`Removed` or `Security`); the rest is the entry text, as it should read in the release notes.
`python3 scripts/changelog.py check` validates them (CI runs it); on a release,
`python3 scripts/changelog.py release X.Y.Z` folds them into `CHANGELOG.md` and deletes them.
