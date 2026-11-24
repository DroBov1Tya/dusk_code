# <img src="projects/clients/desktop/assets/mark.png" alt="" width="40" height="40"> duskcode

An open-source coding agent in Rust. Open the same chats from the desktop app, terminal,
or browser. Chat history is stored locally, and you choose which model provider to use.

[Website](https://drobov1tya.github.io/dusk_code/) ·
[Releases](https://github.com/DroBov1Tya/dusk_code/releases) ·
[Plugin Workshop](https://drobov1tya.github.io/duskcode-workshop/) ·
[Plugins repository](https://github.com/DroBov1Tya/duskcode-workshop)

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh   # download and run the installer
```

The installer asks what to install: the terminal client, the desktop client, or both.
`DUSKCODE_INSTALL=tui`, `gui` or `both` answers ahead, for a server or a script. It
downloads a release build for macOS arm64 or Linux x86_64 / arm64 and checks it against
the digest GitHub keeps for the release.

Open the desktop app or run `duskcode` from a project folder. Run `duskcode --web` to
open the browser client. [Connect a model](#connect-a-model) to start a chat.

`/update` installs a newer release; `duskcode remove` uninstalls and asks first.

<p align="center">
  <img src="docs/screenshots/desktop.png" alt="The duskcode desktop app showing a project chat and its task list" width="900">
</p>

- **One core, thin clients.** The desktop window, the terminal client and the web client
  draw the same threads over one protocol. A feature lands in the core once.
- **Choice of model.** OpenAI, Anthropic, Google, any OpenAI-compatible server,
  local models, or your ChatGPT plan with no API key.
- **Local by default.** Chats are saved in SQLite on your machine. Duskcode needs no
  account, and tracing is off by default. Model requests go to the provider you configure.
- **Readable.** MIT, about one binary per client, agents and skills as Markdown you can edit.

## The clients

| Desktop | Terminal | Web |
| --- | --- | --- |
| <img src="docs/screenshots/desktop-subagents.png" alt="The subagents window: agents by the zone of their work" width="290"> | <img src="docs/screenshots/terminal.png" alt="The terminal client: the rail, the steps of a turn and the answer" width="290"> | <img src="docs/screenshots/web.png" alt="The web client: a chat with the task list beside it" width="290"> |
| `duskcode-gui`: chats and projects, a file viewer and editor, your own terminal in a panel, subagents and the arbiter in windows of their own. macOS and Linux. | `duskcode`: full screen in any terminal, 24-bit colour, keyboard and mouse. Runs on a server over SSH. | `duskcode --web`: the same chats in a browser, served by the core. `--global` opens it to other devices on your network. |

## Connect a model

`/connect` in the terminal, or Settings → AI → LLM in the desktop window.

| How | What it needs |
| --- | --- |
| **Sign in with ChatGPT** | A ChatGPT plan. No API key. |
| OpenAI, Anthropic, Google AI | An API key from the provider's console. |
| Custom | The address of any server that speaks the OpenAI Chat Completions API, and its key if it has one. |
| Local | Ollama, LM Studio, vLLM and the like on `http://localhost`; nothing leaves the machine. |

Several providers can be connected at once; `/model` lists the models of each.

### Sign in with ChatGPT

OpenAI offers this to open-source and locally run apps, and duskcode uses it as
documented:

1. duskcode starts a listener on `127.0.0.1` and opens OpenAI's sign-in page in your
   browser. You sign in there; duskcode never sees your password.
2. OpenAI issues this install a client ID of its own and sends the browser back to the
   listener with a one-time code (authorization code with PKCE; there is no client secret).
3. duskcode exchanges the code for tokens and checks the identity token: its signature
   against OpenAI's published keys, its issuer, audience, expiry and nonce. It also
   checks that use of the plan was granted.
4. Requests go to OpenAI's Responses API with `store: false`, so the conversation is
   not kept there. Usage counts against your plan; you can cap it per app at
   [chatgpt.com/settings/usage](https://chatgpt.com/settings/usage).

The sign-in record is kept where API keys are kept, in the system keychain. It works
only with `api.openai.com`, and removing the provider forgets it.

A Claude subscription cannot be used this way: Anthropic allows signing in with a Claude
account only in its own apps. Claude models work here with an API key.

## What it does

- **Modes**: MAIN for direct work, ORCHESTRATOR to delegate, PLAN to plan first, and
  TEAM, where the agent leads the project like a product owner or team lead: it plans the
  work and runs a team of agents, each with its own role and task, while you set the goal
  and answer its questions. Your own modes are Markdown files.
- **Tools**: read, search and edit files, run commands in the foreground or as background
  jobs, look at pictures, and on a desktop see the screen and use the mouse and keyboard.
- **Subagents**: specialist agents by role, several at once, each in its own thread. An
  arbiter can answer their approval requests under rules you set.
- **Approvals**: ask, sandbox, arbiter or full access, per chat. A command that needs an
  approval nobody can give is denied.
- **Skills, MCP servers and plugins**: skills are loaded when a task needs them; MCP
  servers add tools, with OAuth where the server asks; plugins bring skills, agents,
  providers, hooks, platforms and panels. Each runs in a sandbox, never sees your keys
  and installs in one click from the signed store, the
  [Workshop](https://drobov1tya.github.io/duskcode-workshop/); its plugins, Telegram
  among them, live in [duskcode-workshop](https://github.com/DroBov1Tya/duskcode-workshop)
  ([writing one](docs/plugins.md)).
- **Memory you can read**: notes per project and about you, as plain files. A temporary
  chat is kept in memory only and leaves nothing on disk.
- **Long work**: task lists, goals with acceptance criteria (`/goal`), scheduled runs,
  context compaction (`/compact`), forks from a summary (`/fork`), undo to an earlier
  message.
- **Voice**: dictation and reading aloud through servers you connect or models
  installed on the machine.
- **Headless**: `duskcode run "prompt"` prints the answer; `--json` adds the usage.

### Git

A saved project's repository is managed from the Changes page of the desktop app: stage
or unstage a file, stage all, commit, discard a file's changes (after a yes), make a
branch, fetch, pull (fast-forward only) and push (after a yes, to the upstream or to
`origin` as a new one). The core also merges with a list of conflicts, aborts a merge,
adds and removes worktrees beside the repository, and reads and opens pull requests
through the GitHub CLI when `gh` is installed and signed in, or through a plugin that
serves the remote's host while `plugin_contracts` is on. Either way a pull request on
GitHub is made by `gh pr create`, the GitHub plugin's included; the clients show them
in the Changes module and as chips on pull request links in chat. Git runs with an argument vector and never a shell, with your own hooks,
config and credential helper; the app never reads a token.

It never forces a push, amends a commit, prunes worktrees or removes a worktree that has
uncommitted or untracked files.

## How it is built

```text
 duskcode-gui     duskcode (terminal)     browser
       \                 |                   /
        +--------- dusk_protocol -----------+
                         |
                     dusk_core
        providers · agents · tools · MCP · plugins
        threads in SQLite · secrets in the keychain
```

| Crate | Responsibility |
| --- | --- |
| `projects/protocol` | Requests, responses, events and status every client shares |
| `projects/core` | All logic and state: providers, runtime, tools, agents, config |
| `projects/server` | The way into the core from outside its process: the web server today |
| `projects/clients/terminal`, `desktop`, `browser` | What a person sees: they draw and call the core; the browser page is Leptos, WebAssembly |
| `projects/evals` | Tasks that measure the engine, and their runner |
| `resources/` | Shipped modes, agents, skills, plugins and prompts |

[docs/structure.md](docs/structure.md) says where a file goes;
[docs/features.md](docs/features.md) lists the features and their switches.

## Security and privacy

- **Secrets live in the system keychain** (macOS Keychain, Secret Service on Linux) when
  available. Without one, provider API keys stay in the private config file. Secrets stay
  out of logs, chat history and model context.
- **Network connections.** Duskcode connects to the providers, voice servers and MCP
  servers you configure. It also checks GitHub for updates and fetches the signed catalog
  when you open the Workshop. Tracing is off unless you set an endpoint.
- **Files and git only inside saved projects.** Paths are canonicalized; `..` and
  symlinks that lead out are refused.
- **Model output is data.** It is drawn as Markdown, links open only as http(s), and
  what a file, a page or a tool says is never taken as an instruction to the program.
- **The web client** binds to this machine unless you ask otherwise, signs in with a
  token of the run, checks `Host` and `Origin` on every request, and says plainly that
  the traffic is not encrypted when opened to a network.
- **Shell commands run with your permissions.** Approvals control what is asked first;
  the sandbox mode restricts commands where the system offers a sandbox.

Report a vulnerability as [SECURITY.md](SECURITY.md) says.

## Configuration and data

| Location | Contents |
| --- | --- |
| `~/.config/duskcode/config.yml` | Providers, model, limits, tools, agents, skills, MCP servers, logging |
| `~/.config/duskcode/rules/` | Agents, skills, modes, prompts, plugins and memory, as Markdown |
| `~/.config/duskcode/session/` | The chat database and artifacts |
| `~/.config/duskcode/logs/` | Logs; `logging.enabled: false` switches them off |
| `~/Documents/duskcode/` | Chat workspaces; `projects/` holds project memory |
| `~/.local/share/duskcode/` | The installed program and its resources |

## Build from source

```sh
cargo build --release                 # the terminal client
cargo build --release -p dusk_gui     # the desktop client; needs a recent stable Rust
scripts/build-web.sh                  # the web page, embedded by the next build
```

`Cargo.lock` is committed, so a build uses the dependency versions of a release.

### Keeping the build folder small

Cargo never deletes old artifacts, so `target/` only grows. Give each worktree its own
target folder and never share one `CARGO_TARGET_DIR` between worktrees: each path builds
its own copy of every crate anyway, so a shared folder only piles them up. Delete a
worktree's target folder with the worktree. Now and then run `scripts/clean-build.sh`
from the repository root: it removes what was not used for 7 days (`--days N` to change
that, `cargo sweep` when it is installed), or everything with `--all`.
[CONTRIBUTING.md](CONTRIBUTING.md) has the rules of the code, and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) says how we treat each other.

## Documentation

[docs/structure.md](docs/structure.md), [docs/features.md](docs/features.md),
[docs/web.md](docs/web.md) and [docs/plugins.md](docs/plugins.md) describe the program.

## Limitations

- Pre-1.0: commands, configuration and the database schema may change. Full releases are
  tagged on `stable`, beta pre-releases on `beta`, nightlies on `main`.
  A nightly builds only the programs (terminal, desktop) whose code changed since the
  previous tag (`scripts/release-plan.sh`); its notes name the previous tag for the rest.
- Release builds are for macOS arm64 and Linux; there is no Windows build.
- The macOS app is signed with the project's own certificate, not an Apple Developer
  ID, so the first start needs your confirmation.
- What a model can do depends on the model. Requests to external providers and MCP
  servers fall under their data policies.

## Support

duskcode is free and has no paid tier. If it saves you time and you want to say thanks:

- Bitcoin: `bc1q636n7dg4kwjqn0tm4v5vlxfwzydqgte4423xkz`
- Litecoin: `ltc1q0xdfq0ze6f2cqdufgnnm3p3fls5km9qvjj2vkd`

Issues and pull requests help as much.

## License

[MIT](LICENSE).
