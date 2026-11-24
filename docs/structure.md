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
client/  (21)
  command/  (2)
  hello/  (3)
  request/  (10)
config/  (13)
model/  (7)
plugin/  (13)
  contract/  (4)
project/  (8)
release/  (4)
system/  (3)
text/  (4)
thread/  (34)
  block/  (6)
  message/  (6)
  transcript/  (8)
tools/  (7)
```


### core

```
backup/  (13)
  archive/  (11)
catalog/  (11)
  history/  (4)
client/  (34)
  scope/  (5)
  serve/  (19)
config/  (14)
  file/  (6)
init/  (5)
mcp/  (4)
message/  (18)
  commands/  (9)
  sources/  (6)
platform/  (12)
  linux/  (4)
  macos/  (2)
plugins/  (60)
  broker/  (5)
  contract/  (6)
  platform/  (5)
  store/  (30)
    connector/  (2)
  ui/  (7)
    phone/  (2)
project/  (32)
  episodes/  (4)
  forge/  (9)
  git/  (8)
  ledger/  (4)
providers/  (30)
  anthropic/  (4)
  google/  (5)
  openai/  (6)
  output/  (3)
  transport/  (3)
release/  (8)
runtime/  (129)
  agent/  (37)
    agent/  (16)
    repeats/  (13)
  api/  (18)
  goal/  (10)
    verification/  (7)
  jobs/  (3)
  team/  (22)
    tests/  (8)
  thread/  (34)
    context/  (2)
    runner/  (24)
secrets/  (5)
state/  (67)
  agents/  (4)
  db/  (3)
  project/  (5)
  thread/  (51)
    conversation/  (11)
    goal/  (3)
    work/  (31)
terminal/  (4)
tools/  (74)
  agents/  (4)
  computer/  (21)
    backend/  (5)
    overlay/  (3)
  console/  (12)
    shape/  (7)
  files/  (9)
    map/  (3)
  harness/  (20)
    tasks/  (7)
voice/  (15)
  local/  (7)
```


### clients/desktop

```
columns/  (34)
  inspector/  (18)
    overview/  (3)
  sidebar/  (5)
  terminal/  (4)
  topbar/  (3)
dev/  (6)
modal/  (12)
pages/  (121)
  artifacts/  (3)
  chat/  (59)
    audio/  (4)
    composer/  (6)
    dictation/  (3)
    dock/  (3)
    messages/  (18)
    model_menu/  (4)
  customize/  (24)
    plugins/  (18)
  file/  (2)
  review/  (4)
  schedules/  (3)
  settings/  (22)
    ai/  (6)
    providers/  (6)
  welcome/  (6)
platform/  (4)
plugins/  (10)
ui/  (14)
  theme/  (3)
workspace/  (6)
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
api/  (9)
auth/  (2)
chat/  (29)
  data/  (2)
  inspector/  (8)
  shown/  (4)
columns/  (3)
pages/  (32)
  customize/  (15)
    workshop/  (7)
  settings/  (11)
ui/  (7)
```


The Leptos client, built to wasm with trunk. `link` is the WebSocket, `api` the typed calls by domain,
`state` what the whole page knows, `chat/data` the transcript as plain data (testable without a
browser), `markdown` model output as DOM text only. It draws and calls; it decides nothing.

### clients/terminal

```
app/  (34)
  composer/  (5)
  keys/  (4)
  panels/  (9)
    settings/  (5)
  thread/  (7)
cli/  (7)
host/  (4)
screensaver/  (12)
  scenes/  (6)
session/  (14)
  actions/  (8)
ui/  (58)
  sidebar/  (7)
  text/  (6)
  thread/  (16)
    input/  (8)
    transcript/  (5)
  windows/  (25)
    panels/  (19)
    settings/  (4)
```


A number is the count of Rust files in a directory. `evals` is small and keeps its own layout.

When a file grows past what one reason to change holds, split it the way the existing ones were: a type's `impl` blocks by area into files of a directory module that keeps the type's path (`runtime/api`, `config/file`, `runtime/agent/agent`), tests into a `tests` file or directory beside the code.

### evals

Flat: `args`, `overrides`, `task`, `sandbox`, `score` beside `main.rs`, and `run/`: the runner,
the core it starts on a runtime of its own so a run can kill it (`core`), and where it kills
(`kill`). The tasks are in `projects/evals/tasks/`.
