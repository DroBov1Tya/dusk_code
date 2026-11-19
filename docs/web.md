# The web client

A browser client with the chats, projects, review, connectors and settings of the desktop
app, over the same protocol. Rust only: `projects/clients/browser` is a Leptos 0.8 CSR app built to
wasm with trunk; `projects/server` is the server that embeds the bundle and bridges one
WebSocket per browser to the core.

## Running it

```sh
scripts/build-web.sh                 # wasm32 target, trunk 0.21.14, writes projects/clients/browser/dist
cargo +1.99.1 run -p dusk_tui -- --web            # serves 127.0.0.1:8080
cargo +1.99.1 run -p dusk_tui -- --web --port 9000
```

`--port N` chooses the port (default 8080); a busy port is an error, never a silent switch. `--web` prints an address such as `http://127.0.0.1:8080/#<token>` before the terminal screen
is taken. Open it once. The page reads the token from the fragment (a browser never sends it
anywhere), clears it from the address bar and trades it for a session cookie. The desktop app
does the same from Settings > Advanced > Web access (loopback only).

A build that did not run `scripts/build-web.sh` still compiles; the server then serves a page
that says the client is not built. While working on the client, `projects/server/examples/serve.rs`
serves it over a core of its own, and debug builds read `clients/browser/dist` from disk, so
`trunk build` is enough to see a change.

## One core, several clients

`dusk_core::client::Hub` owns the runtime. Each `Hub::attach` gives a `Client` that receives
the events of every open thread its scope covers; a thread's output has one reader inside the
core, which hands each event to those clients. A thread stays open while any client keeps it
open (`Resume` joins an open thread instead of restarting it; `Close` and going away leave it).
The status is one `watch` every client with the settings in its scope reads. The web server
attaches one client per browser.

## Client scope

Every client is attached with a `Scope` (`core/src/client/scope`). The code that attaches it
chooses the scope; nothing a client sends can change it, and it holds until the client is
dropped. Deny by default, in one place:

- Each request belongs to one area (`Area`: thread reading, thread writing, settings, files,
  terminal and so on; `classify` names every request, so a new one does not compile until it
  has an area). `serve::admit` checks the area before any handler runs.
- `Scope::full()` takes everything: the terminal, the desktop app, the evals runner.
  `Scope::web()` is a list of areas: every thread, and all but the areas named under
  "Security model".
- `Scope::threads(ids, areas)` is bound to those chats and the agent threads below them, so a
  shared chat shows what its subagents do. It takes only requests about its threads, whatever
  its areas: no settings, files, projects, plugins, MCP, terminal, new chats or shutdown, and
  no answer to an approval (an approval does not name its thread yet). Another thread is
  refused as "no such thread", the same as one that does not exist; lists hold its chats only
  and search looks only in them; it gets the events of its chats only, and a blank status
  that never changes. Nothing attaches a client this way yet.
- Thread writing includes the commands sent as a message, which act on that chat alone:
  `/compact`, `/fork`, `/goal` and `/undo <n>`. The scope limits what the client asks the core;
  it never limits the agent's own tools in the chat.
- What answers with more than one chat is not a thread request: the context report (skills,
  MCP tools, usage of every thread) is in the Command area and a chat's diff, which is its
  whole project's, in the Project area, so a bound scope gets neither. A bound scope resumes
  a chat in the folder stored for it, never in one it names.
- Naming a file of this machine is the Command area: `/sources add`, and a path, `~/`,
  `file://`, `@me` or `@snippet` in a sent message. Without the area the message is kept as
  the words it is.
- Still open before a remote client bound to chats may exist: a handler's error text can hold
  paths; refusals are not rate-limited; `Approve { id }` names no thread; the name a client
  gives in `Hello.client` goes to the audit log as it is; the HTTP routes `/api/card` and
  `/api/output` are for the web scope only and check no scope.
- A refusal is one fixed sentence to the client and one warning in the log with the client's
  name and the area, never the request's contents.

## Security model

- Binds 127.0.0.1 by default. Another address needs `--bind` and prints a warning. Reaching it
  from another machine also needs `--allow-host <name:port>` for each name the browser uses.
  The traffic is plain HTTP and the Origin rule accepts `http://` only, so a TLS proxy in front
  of it does not work yet; use it on a network you trust. The token is as strong as a shell on
  the machine: anyone holding it can run commands through the agents.
- A random 256-bit token per run, compared in constant time, exchanged for an opaque session
  id (256 bits, in memory, 12 h idle, at most 32). The cookie is `HttpOnly; SameSite=Strict`,
  and `__Host-` with `Secure` where the page is on a loopback name (browsers accept it there).
  Nothing is kept in `localStorage` except the theme name. Five failed sign-ins in a minute lock
  that TCP peer out (behind a proxy that is everyone). The token works for the whole run. An
  open stream is checked every 20 s: signing out or an expired session ends it.
- `Host` is checked on every request (DNS rebinding); `Origin` on every request that is not a
  plain GET and on the WebSocket upgrade (cross-site WebSocket hijacking), a missing one is
  refused there, and any `Origin` that is present must match.
- Strict CSP (`default-src 'none'`, scripts only from the same origin plus the hash of trunk's
  loader, `wasm-unsafe-eval`, `frame-ancestors 'none'`), `nosniff`, `no-referrer`, no store.
- A frame over 32 MiB or one that is not a request closes the stream; 32 requests at once per
  browser, 16 browsers.
- A browser is attached with `Scope::web()`, and the core refuses what is outside it; the
  bridge itself holds back only a frame over 1 MiB that is not a recording or a picture.
  Refused: `Shutdown` (stopping the core is for the process that started the server), a
  terminal, plugin parts, installing, updating or removing from the Workshop, switching a
  plugin on, writing a file, and reading or editing the config text of a saved MCP server.
  Everything else a signed-in browser can do as the desktop app can, including running
  commands, adding MCP servers and saving projects (parity there, not a boundary).
- Provider keys, plugin secrets and project variables never reach the page: the page only sends
  new values (password inputs) and shows whether one is set. Known gap: the text of an MCP
  server's config, opened for editing, includes its `env` and `headers` values.
- Model output is markdown drawn with DOM text nodes: raw HTML is shown as text, links are
  http(s) only and open with `noopener`, pictures in markdown are not fetched.
- Cards are served by ID from the artifact folder (typed by their bytes); outputs by a path the
  core's preview rule allows, and only as pictures.
- A browser that goes away does not stop its running turn at once: a thread nobody keeps open
  gets 60 s to be picked up again (a reload), then it is stopped.

## Parity with the desktop app

Done = works in the browser and was checked against a real core. Computer use is not part of it.

- [x] Chat: streaming, markdown, reasoning fold, tool steps folded, images and sounds
- [x] Queue, send now, stop, slash commands with suggestions, queued messages shown
- [x] Approvals (once / for the chat / deny), questions, per-chat access, mode, context meter
- [x] Subagents: tile with the Sundown mascot, window with the agent's own folded transcript, stop
- [x] Inspector: plan (edit), tasks, agents, changed files with diff, jobs with output, context
- [x] Projects: add, edit, remove, git (branches, changes, review), files, environment names
- [x] Artifacts gallery with picture previews, text viewer, delete
- [x] Connectors: agents (edit), skills (edit), MCP (add, edit, restart, sign in), plugins
      with secrets, tools, voice (models install, cancel, remove; other servers)
- [x] Settings: appearance (5 themes), model and providers, chat, safety, secrets, about me,
      notifications, version control, memory and context, advanced, updates, usage
- [x] Dictation (microphone to a 16 kHz WAV, `Transcribe`) and reading answers aloud
- [x] New chat draft: the thread is made with the first message
- [ ] Attachments and pasted files in the composer, `@` mention completion
- [ ] Pinned chats, chat context menu beyond rename and close, keyboard page and shortcuts
- [ ] Fork button (the `/fork` command works), undo points list
- [ ] Browser notifications and sounds when a chat needs you
- [ ] Uninstall and restore (they are for the machine, not for a browser)
- [ ] Picking a project folder with a browser dialog (the path is typed; it is a path on the
      machine that runs the core)

## Known limits

Event queues per client are unbounded (a browser that accepts a frame every few seconds is
kept); every request frame may be up to 32 MiB, because a recording is the largest one; the
card routes check a file and then read it (an agent that can write the artifact folder could
race a symlink in); an image request attaches a short-lived client to the core.

## Checked and not checked

Checked in the built-in browser against a real core with a mock provider: sign-in by fragment,
a stale session returning to sign-in, streaming, two tabs seeing one chat live, approval, tool
steps, project and git pages, connectors and settings pages. `cargo test` covers the host and
origin rules, auth and its lockout, frame limits, the multi-client fan-out and the frame round
trips. Not checked: microphone capture and playback in a browser, local model installs, the
desktop switch at runtime, Safari (it does not store `Secure` cookies over http), a
non-loopback bind from another machine.
