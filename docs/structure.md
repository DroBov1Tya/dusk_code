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
catalog/  (6)
client/  (22)
  command/  (2)
  hello/  (3)
  request/  (10)
config/  (14)
model/  (7)
plugin/  (15)
  contract/  (4)
project/  (8)
release/  (4)
system/  (3)
text/  (4)
thread/  (52)
  block/  (6)
  message/  (6)
  team/  (6)
  team_layout/  (5)
  transcript/  (14)
    log/  (5)
tools/  (7)
```


### core

```
backup/  (13)
  archive/  (11)
catalog/  (11)
  history/  (4)
client/  (38)
  scope/  (5)
  serve/  (22)
config/  (14)
  file/  (6)
init/  (5)
mcp/  (4)
message/  (18)
  commands/  (9)
  sources/  (6)
net/  (3)
platform/  (12)
  linux/  (4)
  macos/  (2)
plugins/  (63)
  broker/  (5)
  contract/  (6)
  platform/  (5)
  store/  (33)
    connector/  (4)
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
runtime/  (138)
  agent/  (42)
    agent/  (17)
    repeats/  (17)
  api/  (19)
    plugins/  (1)
  goal/  (5)
  jobs/  (3)
  team/  (26)
    tests/  (10)
  thread/  (37)
    context/  (2)
    runner/  (27)
secrets/  (5)
state/  (73)
  agents/  (5)
  db/  (3)
  project/  (5)
  thread/  (56)
    conversation/  (11)
    goal/  (3)
    work/  (34)
terminal/  (4)
tools/  (85)
  agents/  (4)
  computer/  (21)
    backend/  (5)
    overlay/  (3)
  console/  (18)
    rerun/  (5)
    shape/  (7)
  files/  (9)
    map/  (3)
  harness/  (25)
    tasks/  (7)
    workshop/  (4)
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
dev/  (9)
modal/  (14)
pages/  (141)
  artifacts/  (3)
  chat/  (67)
    activity/  (4)
    audio/  (4)
    composer/  (7)
    dictation/  (3)
    dock/  (4)
    messages/  (20)
    model_menu/  (4)
  customize/  (25)
    plugins/  (19)
  file/  (2)
  review/  (4)
  schedules/  (3)
  settings/  (23)
    ai/  (6)
    providers/  (6)
  team/  (7)
  welcome/  (6)
platform/  (4)
plugins/  (10)
ui/  (15)
  theme/  (3)
workspace/  (7)
```


### server

```
auth/  (4)
files/  (4)
local/  (9)
  version/  (1)
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
chat/  (32)
  data/  (2)
  inspector/  (8)
  shown/  (4)
columns/  (3)
dev/  (1)
pages/  (36)
  customize/  (16)
    workshop/  (8)
  settings/  (12)
  welcome/  (3)
ui/  (7)
```


The Leptos client, built to wasm with trunk. `link` is the WebSocket, `api` the typed calls by domain,
`state` what the whole page knows, `chat/data` the transcript as plain data (testable without a
browser), `markdown` model output as DOM text only, `dev` made-up chats in debug builds only. It draws and
calls; it decides nothing.

### clients/terminal

```
app/  (36)
  composer/  (6)
  keys/  (4)
  panels/  (9)
    settings/  (5)
  thread/  (8)
    transcript/  (1)
cli/  (7)
host/  (4)
screensaver/  (12)
  scenes/  (6)
session/  (15)
  actions/  (8)
ui/  (59)
  sidebar/  (7)
  text/  (6)
  thread/  (17)
    input/  (8)
    transcript/  (6)
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
