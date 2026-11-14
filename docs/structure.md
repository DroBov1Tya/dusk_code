# How the code is laid out

The workspace is one core and thin clients over one protocol. This file says where a crate
goes and how one is built inside, so a new file has an obvious home.

## Workspace

```text
projects/
  protocol/          the vocabulary of the system; depends on nothing
  core/              the engine: all logic and state
  server/            the way into the core from outside its process: browsers today,
                     paired devices later
  clients/           what a person sees; they draw and ask, nothing else
    terminal/        `duskcode`
    desktop/         `duskcode-gui`
    browser/         the web page, built to wasm and embedded by the server
  plugins/
    telegram/        the program of the built-in telegram plugin
  evals/             tasks that measure the engine, and their runner
```

Dependencies point one way, down this tree: `clients -> server -> core -> protocol`. A
crate never reaches up it. A new client goes under `clients/`, a new program a plugin
ships under `plugins/`; nothing new goes at the root of `projects/`.

## Rules

1. **A module is a place with a front door.** A directory module has a `mod.rs` that says in
   its doc what the module is for, declares its submodules and re-exports the items others
   use, flat. Callers write `dusk_protocol::thread::Message`, never the path of the file it
   lives in. The files behind the door are free to move.
2. **One reason to change per file.** When a file grows a second reason to change, it
   becomes a directory with a file for each. A file past about 400 lines is a question; past
   800 it is an answer. `impl` blocks of one type may be spread over files, one per area
   (see `core/src/runtime/api`).
3. **Domains over layers.** Group by what a thing is about (`thread`, `config`, `project`),
   not by what it is (`types`, `utils`, `helpers`). A grab bag is a missing domain.
4. **Top level is a map.** The root of a crate holds its modules and the few files that
   start it (`lib.rs`, `main.rs`, `app.rs`). A feature does not add a file at the root: it
   goes into the module it belongs to, or becomes a module with a reason to exist.
5. **The protocol is a framework.** Its domains are the vocabulary of the whole system; the
   core and every client use the same words. A new request goes into `client/request/`
   under the area it belongs to, a new event field into the domain it is about.
6. **Clients draw, the core decides.** The clients keep presentation only; anything that
   decides, stores or checks lives in the core and reaches them through the protocol.
7. **Moves are mechanical and checked.** A restructure changes paths and nothing else: no
   behavior, no renames of types, one commit that builds and passes tests.

## Map

Regenerate with `scripts/structure-map.sh` in the commit that changes the tree.

### protocol

```
agent/  (2)
catalog/  (5)
client/  (18)
  command/  (2)
  request/  (9)
config/  (12)
model/  (6)
project/  (6)
release/  (4)
system/  (3)
thread/  (16)
  message/  (6)
tools/  (6)
```

### core

```
backup/  (12)
  archive/  (11)
catalog/  (6)
client/  (21)
  serve/  (11)
config/  (12)
  file/  (6)
init/  (5)
mcp/  (4)
message/  (17)
  commands/  (9)
  sources/  (6)
platform/  (18)
  linux/  (2)
  macos/  (10)
    simulator/  (8)
plugins/  (10)
  platform/  (5)
project/  (8)
providers/  (29)
  anthropic/  (4)
  google/  (5)
  openai/  (6)
  output/  (3)
  transport/  (3)
release/  (8)
runtime/  (63)
  agent/  (16)
    agent/  (7)
  api/  (16)
  goal/  (10)
    verification/  (7)
  jobs/  (3)
  thread/  (13)
    runner/  (6)
secrets/  (5)
state/  (32)
  agents/  (4)
  db/  (3)
  project/  (3)
  thread/  (19)
    conversation/  (7)
    goal/  (3)
    work/  (5)
terminal/  (4)
tools/  (52)
  agents/  (4)
  computer/  (18)
    backend/  (5)
    overlay/  (3)
  console/  (6)
  files/  (4)
  harness/  (11)
voice/  (15)
  local/  (7)
```

### clients/desktop

```
columns/  (34)
  inspector/  (13)
  sidebar/  (4)
  simulator/  (6)
  terminal/  (4)
  topbar/  (3)
dev/  (2)
modal/  (11)
pages/  (79)
  artifacts/  (3)
  chat/  (41)
    audio/  (4)
    composer/  (5)
    dictation/  (3)
    messages/  (7)
    model_menu/  (4)
  connectors/  (12)
    voice/  (5)
  file/  (2)
  review/  (3)
  schedules/  (3)
  settings/  (14)
    providers/  (6)
platform/  (3)
ui/  (7)
  theme/  (2)
workspace/  (5)
```

### server

```
auth/  (4)
files/  (4)
```

The library the terminal and desktop clients link to serve browsers; it has no binary of its own and the
built bundle is embedded in each of them. `auth` (token, sessions, limit), `files` (`assets`:
the bundle embedded from `clients/browser/dist`; `cards`, `outputs`: pictures and sounds), `guard`
(host, origin, security headers), `bridge` (one WebSocket per browser), `server` (routes,
start, stop). Everything else is the core's.

### clients/browser

```
api/  (8)
auth/  (2)
chat/  (18)
  data/  (2)
  inspector/  (7)
columns/  (2)
pages/  (24)
  connectors/  (8)
  settings/  (11)
ui/  (5)
```

The Leptos client, built to wasm with trunk. `link` is the WebSocket, `api` the typed calls by domain,
`state` what the whole page knows, `chat/data` the transcript as plain data (testable without a
browser), `markdown` model output as DOM text only. It draws and calls; it decides nothing.

### clients/terminal

```
app/  (31)
  composer/  (5)
  keys/  (3)
  panels/  (8)
    settings/  (5)
  thread/  (7)
cli/  (7)
host/  (5)
screensaver/  (12)
  scenes/  (6)
session/  (12)
  actions/  (7)
ui/  (50)
  sidebar/  (7)
  text/  (5)
  thread/  (14)
    input/  (8)
    transcript/  (3)
  windows/  (20)
    panels/  (14)
    settings/  (4)
```

A number is the count of Rust files in a directory. `evals` and `external/telegram` are small and keep their own layout.

When a file grows past what one reason to change holds, split it the way the existing ones were: a type's `impl` blocks by area into files of a directory module that keeps the type's path (`runtime/api`, `config/file`, `runtime/agent/agent`), tests into a `tests` file or directory beside the code.

### plugins/telegram

```
app/  (13)
  bot/  (9)
```

### evals

Flat: `args`, `run`, `task`, `sandbox`, `score` beside `main.rs`; the tasks are in
`projects/evals/tasks/`.
