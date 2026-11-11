# The web client

A browser client with the chats, projects, review, connectors and settings of the desktop
app, over the same protocol. Rust only: `projects/web-ui` is a Leptos 0.8 CSR app built to
wasm with trunk; `projects/web` is the server that embeds the bundle and bridges one
WebSocket per browser to the core.

## Running it

```sh
scripts/build-web.sh                 # wasm32 target, trunk 0.21.14, writes projects/web-ui/dist
cargo +1.99.1 run -p dusk_tui -- --web            # serves 127.0.0.1:8080
cargo +1.99.1 run -p dusk_tui -- --web --port 9000
```

`--port N` chooses the port (default 8080); a busy port is an error, never a silent switch. `--web` prints an address such as `http://127.0.0.1:8080/#<token>` before the terminal screen
is taken. Open it once. The page reads the token from the fragment (a browser never sends it
anywhere), clears it from the address bar and trades it for a session cookie. The desktop app
does the same from Settings > Advanced > Web access (loopback only).

A build that did not run `scripts/build-web.sh` still compiles; the server then serves a page
that says the client is not built. While working on the client, `projects/web/examples/serve.rs`
serves it over a core of its own, and debug builds read `web-ui/dist` from disk, so
`trunk build` is enough to see a change.

## One core, several clients

`dusk_core::client::Hub` owns the runtime. Each `Hub::attach` gives a `Client` that receives
the events of every open thread; a thread's output has one reader inside the core, which hands
each event to every attached client. A thread stays open while any client keeps it open
(`Resume` joins an open thread instead of restarting it; `Close` and going away leave it).
The status is one `watch` every client reads. The web server attaches one client per browser.

## Security model

- Binds 127.0.0.1 by default. Another address needs `--bind` (and `--allow-host` for names
  behind a proxy) and prints a warning: the traffic is plain HTTP, put TLS in front of it.
- A random 256-bit token per run, compared in constant time, exchanged for an opaque session
  id (256 bits, in memory, 12 h idle, at most 32). The cookie is `HttpOnly; SameSite=Strict`,
  and `__Host-` with `Secure` where the page is on a loopback name (browsers accept it there).
  Nothing is kept in `localStorage` except the theme name. Five failed sign-ins in a minute lock
  that peer out.
- `Host` is checked on every request (DNS rebinding); `Origin` on every POST and on the
  WebSocket upgrade (cross-site WebSocket hijacking), and a missing one is refused there.
- Strict CSP (`default-src 'none'`, scripts only from the same origin plus the hash of trunk's
  loader, `wasm-unsafe-eval`, `frame-ancestors 'none'`), `nosniff`, `no-referrer`, no store.
- A frame over 32 MiB or one that is not a request closes the stream; 32 requests at once per
  browser, 16 browsers.
- The bridge refuses `Shutdown`: stopping the core is for the process that started the server.
- Secrets never reach the page: the protocol does not carry them out; the page only sends
  new values (password inputs) and shows whether one is set.
- Model output is markdown drawn with DOM text nodes: raw HTML is shown as text, links are
  http(s) only and open with `noopener`, pictures in markdown are not fetched.
- Cards and outputs are served by ID or by a path the core's own preview rule allows, and only
  as pictures or sounds.

## Parity with the desktop app

Done = works in the browser and was checked against a real core. Computer use is not part of it.

- [x] Chat: streaming, markdown, reasoning fold, tool steps folded, images and sounds
- [x] Queue, send now, stop, slash commands with suggestions, queued messages shown
- [x] Approvals (once / for the chat / deny), questions, per-chat access, mode, context meter
- [x] Subagents: tile with fox badge, window with the agent's own folded transcript, stop
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

## Checked and not checked

Checked in the built-in browser against a real core with a mock provider: sign-in by fragment,
a stale session returning to sign-in, streaming, two tabs seeing one chat live, approval, tool
steps, project and git pages, connectors and settings pages. `cargo test` covers the host and
origin rules, auth and its lockout, frame limits, the multi-client fan-out and the frame round
trips. Not checked: microphone capture and playback in a browser, local model installs, the
desktop switch at runtime, Safari (it does not store `Secure` cookies over http), a
non-loopback bind from another machine.
