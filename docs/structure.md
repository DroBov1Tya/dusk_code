# How the code is laid out

The workspace is one core and thin clients over one protocol. This file says how a crate is
built inside, so a new file has an obvious home.

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
6. **Clients draw, the core decides.** `gui` and `tui` keep presentation only; anything that
   decides, stores or checks lives in the core and reaches them through the protocol.
7. **Moves are mechanical and checked.** A restructure changes paths and nothing else: no
   behavior, no renames of types, one commit that builds and passes tests.

## Map

### protocol

```
agent/  (2)
catalog/  (5)
client/  (16)
  command/  (2)
  request/  (8)
config/  (11)
model/  (6)
project/  (7)
release/  (4)
system/  (2)
thread/  (16)
  message/  (6)
tools/  (5)
```

### core

```
backup/  (12)
  archive/  (11)
catalog/  (6)
client/  (16)
  serve/  (10)
config/  (14)
  file/  (6)
init/  (4)
mcp/  (4)
message/  (12)
  commands/  (9)
platform/  (8)
  linux/  (2)
  macos/  (2)
plugins/  (6)
project/  (7)
providers/  (23)
  anthropic/  (4)
  openai/  (6)
  output/  (3)
  transport/  (3)
release/  (8)
runtime/  (60)
  agent/  (15)
  api/  (16)
  goal/  (10)
  jobs/  (3)
  thread/  (5)
  thread_runner/  (5)
secrets/  (4)
state/  (32)
  agents/  (4)
  db/  (3)
  thread/  (18)
terminal/  (4)
tools/  (49)
  agents/  (4)
  computer/  (16)
  console/  (6)
  files/  (4)
  harness/  (11)
util/  (5)
voice/  (8)
  local/  (7)
```

### gui

```
columns/  (19)
  inspector/  (12)
  sidebar/  (4)
modal/  (11)
pages/  (53)
  artifacts/  (1)
  chat/  (23)
  connectors/  (10)
  schedules/  (2)
  settings/  (8)
platform/  (3)
ui/  (5)
  theme/  (2)
```

### web

```
auth/  (4)
files/  (4)
```

The library both `tui` and `gui` link to serve browsers; it has no binary of its own and the
built bundle is embedded in each of them. `auth` (token, sessions, limit), `files` (`assets`:
the bundle embedded from `web-ui/dist`; `cards`, `outputs`: pictures and sounds), `guard`
(host, origin, security headers), `bridge` (one WebSocket per browser), `server` (routes,
start, stop). Everything else is the core's.

### web-ui

```
api/  (8)
chat/  (14)
  data/  (2)
  inspector/  (7)
pages/  (28)
  connectors/  (8)
  settings/  (11)
```

The Leptos client, built to wasm with trunk. `link` is the WebSocket, `api` the typed calls by domain,
`state` what the whole page knows, `chat/data` the transcript as plain data (testable without a
browser), `markdown` model output as DOM text only. It draws and calls; it decides nothing.

### tui

```
app/  (30)
  composer/  (5)
  keys/  (3)
  panels/  (8)
  thread/  (7)
cli/  (6)
host/  (5)
screensaver/  (12)
  scenes/  (6)
session/  (5)
ui/  (43)
  text/  (5)
  thread/  (14)
  windows/  (19)
```

A number is the count of Rust files in a directory. `evals` and `external/telegram` are small and keep their own layout.

When a file grows past what one reason to change holds, split it the way the existing ones were: a type's `impl` blocks by area into files of a directory module that keeps the type's path (`runtime/api`, `config/file`, `runtime/agent/agent`), tests into a `tests` file or directory beside the code.
