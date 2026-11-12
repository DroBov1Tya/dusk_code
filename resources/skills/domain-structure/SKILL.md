---
name: domain-structure
description: "Lay out and keep a codebase as a tree of domains where every path reads like a framework call and every file has one reason to change. Use when adding a file or module, deciding where code goes, splitting a large file, restructuring or auditing a project's layout, reviewing structure in a change, or starting a new project; any language."
source: "dusk_code"
author: "Neil Ermak"
license: MIT
tags: [project, architecture]
---
# Domain structure

Keep a codebase readable from its paths alone. A newcomer opens the tree, reads the
names, and knows where a thing lives before opening a file. Follow the project's own
conventions and its structure document first; apply these rules where they are silent.

## The model

- **Package, domain, subdomain.** A package (crate, npm package, Python package) is a
  tree of domains; a domain may hold subdomains. A path is a sentence:
  `core::runtime::agent::compaction`, `billing/invoice/pdf`. Two or three levels are
  usual; a fourth needs a reason.
- **A front door per module.** Each directory has one entry file (`mod.rs`, `index.ts`,
  `__init__.py`) holding a one-paragraph doc of what the module is for, its submodules,
  and flat re-exports of what others use. Callers import from the door
  (`protocol::thread::Message`), never from the file behind it. Files behind a door are
  free to move. The door holds no implementation.
- **One reason to change per file.** A file answers to one decision. Several internal
  steps of one action (parse, validate, write for a single `save`) are one reason.
- **Domains over layers.** Group by what a thing is about (`thread`, `invoice`,
  `project`), not by what it is (`types`, `models`, `handlers`, `services`, `utils`,
  `helpers`, `common`). A grab bag is a domain nobody named yet.
- **The top level is a map.** A package root holds its domains and the few files that
  start it (`lib.rs`, `main.rs`, `index.ts`, `__main__.py`). A feature never adds a
  root file.
- **A contract package is the vocabulary.** With several programs, one package holds
  the requests, responses, events and shared data. Everyone uses its words; its domains
  mirror the core's.
- **Clients draw, the core decides.** Anything that decides, stores, checks or parses
  user intent lives in the core and reaches clients through the contract. A client
  keeps presentation: layout, themes, which panel is open. The same rule written in two
  clients is a missing core function.

## Adding code

1. Name what the code is about in one or two words. That is its domain. Find it in the
   map; search for the nearest existing code before creating anything.
2. Pick the file by asking what would make this code change. If an existing file
   changes for the same reason, add it there. If the file has another reason, do not
   widen it.
3. Second-responsibility test. Describe the file in one sentence without "and". If you
   cannot, or if two people could edit it in one week for unrelated features, it has two
   responsibilities: make it a directory with the file's name, one file per
   responsibility, and a door that keeps the old import path. Steps of one action do
   not count; a length past about 400 lines is a question, past 800 an answer.
4. New domain or extend one. Create a domain when the thing has its own nouns, its own
   state or lifecycle, and at least two files you can name today. Otherwise extend the
   closest domain. Never create a directory for one file or for a future that has not
   arrived.
5. Shared code. Something used by two domains goes to the domain that owns the concept
   and is re-exported from its door. If it is about the machine, it goes to `platform`;
   about files on disk, to the module that owns those files; about text shown to people,
   to the presentation domain. If you still want `utils`, write down what the helpers
   have in common: that word is the module's name.
6. Name the file for what it does: a noun for a thing (`invoice.rs`, `lock.rs`), a verb
   phrase for an action (`send.rs`, `run_command.rs`). Tests sit beside the code in a
   `tests` file or directory.

Names that say what a file does: `compaction.rs`, `path_completion.rs`, `rate_limit.ts`,
`atomic_write.py`, `approval.rs`. Names that do not: `utils.rs`, `helpers.ts`,
`common.py`, `misc`, `shared`, `types.ts`, `manager.rs`, `service.py`, `data.rs`,
`core2`, `new_`, `stuff`.

## Worked examples

A good tree (Rust; the same shape holds elsewhere):

```
protocol/src/          the contract: thread/ project/ config/ client/request/
core/src/
  lib.rs               doc: one line per domain
  config/  mod.rs  file/  live.rs
  project/ mod.rs  git.rs  diff.rs  files.rs
  runtime/ mod.rs  agent/  thread/  goal/
  state/   mod.rs  thread/  project/
tui/src/  gui/src/     ui/ pages/ ... presentation only
```

Splitting a file with two responsibilities, import path unchanged:

```
before  message/sources.rs       stores pasted sources AND decodes and shrinks pictures
after   message/sources/mod.rs   doc, `pub use store::ThreadSources; pub use picture::picture;`
        message/sources/store.rs
        message/sources/picture.rs
        message/sources/tests.rs
```

The same rules in three languages:

| | Rust | TypeScript | Python |
|---|---|---|---|
| Package | crate | workspace package | distribution package |
| Front door | `mod.rs` with `//!` doc and `pub use` | `index.ts` with a header comment and `export { X } from "./x"` | `__init__.py` with a docstring, imports and `__all__` |
| Hidden files | private `mod x;` | not listed in `exports`; lint against deep imports | leading underscore or absent from `__all__` |
| Caller writes | `core::project::Diff` | `import { Diff } from "@app/core/project"` | `from app.project import Diff` |
| Layer smell | `types.rs`, `util/` | `components/ hooks/ services/ types/` at the root | `models.py views.py utils.py` holding every feature |
| Domain fix | `project/diff.rs` | `project/{Diff.tsx,useDiff.ts,api.ts,index.ts}` | `project/{diff.py,api.py,__init__.py}` |

Framework-mandated folders (`app/` routes, `migrations/`) stay; put domains inside or
beside them and keep the mandated files thin.

## Audit

1. List every directory with its file count, and every source file with its line count,
   sorted by size. Read each front door and the first lines of suspicious files; read
   excerpts, not whole files.
2. Flag: root files that are features; grab-bag modules; files past about 400 lines
   with more than one reason to change (name the reasons); directories with no door doc
   or with one file; imports that reach past a door; layer-named groups; a concept
   implemented twice, above all logic copied between clients; names that do not say
   what the file does; a map document that disagrees with the tree.
3. Rank: high for a wrong domain or duplicated logic, medium for a file to split, low
   for naming and docs. Give each finding a path, the rule it breaks, and the target
   path or split. Report the 15 to 25 most valuable; do not dump every file.
4. Change nothing during an audit. Separate mechanical moves from findings that need
   behavior to move between packages; those are features, not restructures.

## Restructure

- Moves only: paths, module declarations, imports, re-exports. No behavior change, no
  type or function renames, no formatting sweep, no fixes noticed on the way; note
  those for later.
- Keep old import paths working through the door when callers are many; remove the
  alias in a later change.
- Move with the version-control move command so history follows. One commit per
  restructure that builds, passes lint and the full tests. Check the working tree first;
  never move files others are editing without agreement.
- Update the map document in the same commit. Ask before a restructure that touches
  many packages or public import paths.

## The map: `structure.md`

Keep one short document (`docs/structure.md` or the project's equivalent) with: the
rules in the project's words; for each package a tree of its directories two levels
deep with a file count; one line for each root file and each domain whose name does not
explain itself; where tests live; the known exceptions and why. No file lists, no
function names. Update it in the commit that adds, removes, renames or splits a
directory; an edit inside a file never touches it. When the map and the tree disagree,
the tree is right and the map is a bug to fix now.

## Before finishing a change

- Every new file sits in a domain; nothing new at a package root.
- Each touched file still fits one sentence without "and".
- No new `utils`, `helpers`, `common`, `types`, `misc`.
- New directories have a door with a doc paragraph and flat re-exports; no caller
  imports past a door.
- No decision, storage, check or parsing added to a client; nothing copied from another
  client.
- Shared vocabulary went into the contract package first.
- Names say what the file does; tests sit beside the code.
- The map document matches the tree.
- Build, lint and tests ran; say what was run and what was not.
