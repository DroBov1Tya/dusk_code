# Plugins

A plugin adds to duskcode without changing its code. It is a folder with a
`plugin.yml` under `~/.config/duskcode/rules/plugins/`. duskcode reads the folder at
start and again every minute, so a plugin that is dropped in, edited or removed
applies without a restart. A plugin that is found stays off until you switch it on in
`/plugins`, because a plugin runs programs of its own.

The core never loads plugin code into its own process. A plugin is declarations plus
separate processes, so a plugin can be written in any language and cannot crash the
core.

## Layout

```text
rules/plugins/my-plugin/
├── plugin.yml
├── skills/      # optional: skills, installed as skills/my-plugin/...
├── agents/      # optional: agents, installed as agents/my-plugin/...
└── ...          # its programs: MCP servers, hooks, a platform
```

## plugin.yml

```yaml
name: my-plugin            # the same as the folder
version: 0.1.0
duskcode_version: "0.74.2"  # optional: Dusk Code version used to write the plugin
description: What it brings, in a line.
platforms: [macos, linux]  # systems its programs are built for: macos | linux | windows; all when left out
clients: [gui, tui]        # clients that can draw its parts: gui | tui | web; all when left out
settings: [token]          # settings it needs, given under plugins: in config.yml

providers:                 # model providers on the protocols the core speaks
  local:
    kind: openai_compatible
    base_url: http://127.0.0.1:8080/v1

mcp:                       # MCP servers: the way to add tools
  search:
    command: ./search-server   # ./ is the plugin's folder

hooks:                     # programs run on thread events
  - event: before_tool     # turn_start, user_message, before_tool, after_tool, turn_end
    tools: [run_command]   # tool events only; every tool when left out
    command: [./check.sh]
    timeout: 10            # seconds, 1 to 600

platform:                  # a program the core keeps running, such as a chat bot
  command: [./bot]
```

On a system not in `platforms` the plugin is listed but cannot be switched on, and the
store shows why. A client not in `clients` does not show its parts.
The plugin list or details show `duskcode_version` when present. This field is informational.

Unknown fields are refused, so a typo shows in `/plugins` as an error instead of a
part that silently does nothing. When a name in `providers` or `mcp` is also in your
`config.yml`, your config's entry wins.

## Settings

Each plugin can be given settings, such as a token, in `config.yml`:

```yaml
plugins:
  my-plugin:
    enabled: true
    settings:
      token: "..."
```

A plugin that lacks a setting it lists stays off, and `/plugins` says which setting is
missing. Every program of a plugin gets these environment variables:

| Variable | Contents |
| --- | --- |
| `DUSKCODE_PLUGIN_DIR` | the plugin's folder |
| `DUSKCODE_PLUGIN_DATA` | a private folder for the plugin's own state, kept across updates |
| `DUSKCODE_PLUGIN_SETTINGS` | its settings as a JSON object |

## Skills and agents

When the plugin is on, files under `skills/` and `agents/` are copied to
`rules/skills/<plugin>/` and `rules/agents/<plugin>/`, where duskcode reads its own.
When the plugin changes, updated files are copied again. A file you edited after it
was copied is never replaced or removed. When the plugin is switched off, the files it
copied go away, except the ones you edited.

## Hooks

A hook reads one JSON object on stdin:

```json
{"event": "before_tool", "thread": "…", "depth": 0, "tool": "run_command", "arguments": {"command": "ls"}}
```

`after_tool` also carries the tool's `result`; `user_message` carries `text`;
`turn_end` carries `status`. The hook can print one JSON object back:

- `{"decision": "deny", "reason": "…"}` keeps a tool from running (`before_tool` only);
- `{"context": "…"}` adds a note the model reads: to the tool's result after a tool, or
  to the thread for the other events.

A call that runs other tools inside it, such as `computer_actions`, is checked first
as itself and then once per step, with that step's `tool` and `arguments` and
`"within": {"tool": "computer_actions", "step": 2}`, so a hook for `type_text` sees
typing in a batch too. A deny from any step refuses the whole call before anything is
asked or done. `after_tool` runs once, for the batch, with every step's result in it.

Printing nothing allows the action. Hooks run in the order the plugins are named. A
`before_tool` hook that fails, prints something other than JSON or runs past its
timeout denies the call: a check that could not run has not allowed anything. Other
hooks that fail are logged and skipped.

## Platforms

A platform brings you to duskcode from somewhere else, such as a chat. The core starts
it, starts it again if it ends, and gives it a thread of its own: the newest one it had,
or a new one. They exchange one JSON object per line.

From the core to the platform:

| Message | Meaning |
| --- | --- |
| `{"type": "answer", "text": "…"}` | the agent's final answer, in Markdown |
| `{"type": "question", "question": "…", "options": […], "multiple": false}` | the agent asks |
| `{"type": "approval", "id": 3, "text": "…"}` | a tool waits for a yes |
| `{"type": "reply", "text": "…"}` | a short answer to a command, or a note |
| `{"type": "menu", "command": "mode", "title": "…", "options": […]}` | what a command can be set to |
| `{"type": "image", "path": "/…", "mime_type": "image/png", "caption": "…"}` | an image card the agent showed, by its file |
| `{"type": "working", "on": true}` | the agent started or stopped working |

From the platform to the core:

| Message | Meaning |
| --- | --- |
| `{"type": "message", "text": "…"}` | a message, or the answer to an open question |
| `{"type": "chosen", "options": [0, 2]}` | options picked for the open question |
| `{"type": "approval", "id": 3, "allow": true}` | the answer to a request |
| `{"type": "command", "name": "status", "argument": null}` | stop, resume, status, approve, deny, mode, model, compact, goal or new |
| `{"type": "status", "text": "…"}` | what `/plugins` shows, such as a code to pair with |

Whoever reaches the platform acts as you, so the platform decides who that is. The
Telegram plugin, for example, answers only the one user who paired with it.

## Permissions and isolation

A plugin's programs never see what the core keeps: provider keys, sign-in tokens and
the keychain are out of their reach, and a plugin gets nothing it does not ask for.

- **Environment.** Every program a plugin runs starts with an empty environment plus
  `PATH`, `HOME`, the locale, `TMPDIR`, the `DUSKCODE_PLUGIN_*` variables and the proxy
  below. No provider key travels in an environment variable.
- **Files.** A program writes only in `DUSKCODE_PLUGIN_DATA` and the temporary folder. It
  reads its own folder, but not the rest of `~/.config/duskcode`, other plugins or the
  keychain.
- **Network.** A program cannot connect anywhere itself. Its requests go through the
  core, at `DUSKCODE_PLUGIN_PROXY` (also set as `HTTPS_PROXY`), which lets them out only
  to the origins the manifest lists, and never to private or local addresses. The list
  limits where a program connects, not what it sends there: anything it can read (its
  own folder, a project it is asked about, a token file its tool keeps) it can send to
  those origins, under any account it can use there. Switching a plugin on is trusting
  it with what it reads.
- **Keys.** A plugin that needs a key for its own service names it and where it may go.
  The core keeps the key and puts it into requests to those origins only: the plugin
  writes `{{key:token}}` in a URL or a header and never sees the value. A key that must
  be read by the plugin itself, for a protocol the core cannot carry, is marked
  `handed`, and you are told plainly that the plugin will see it. Even then it can only
  send it to the origins it may reach.
- **Your keys.** The keys of your providers and your sign-ins are never given to a
  plugin, by any message or setting.

```yaml
permissions:
  network: [https://api.example.com]     # origins, through the core
  exec: [xcrun, axe]                     # programs it may start besides its own;
                                         # xcrun brings the developer tools of Xcode
  read: [~/Library/Developer]            # folders outside its own it may read
  sockets: [~/.vphone]                   # folders whose Unix sockets it may connect to
  keys:
    - name: token
      origins: [https://api.example.com]
      handed: false                      # true: the plugin reads the key itself
  project: read                      # only with provides: the folder of the project a
                                     # call is about, for that call (none | read)
```

`project: read` exists only for a plugin that provides a contract (see Contracts): it
lets its program read the one project folder a call is about, while it answers, and
never another project's. You see it as "read the folder of the project it is asked
about, while it answers". A plugin that may connect and read is also shown "send what it
reads to those addresses, under any account it can use there". No plugin reads the
keychain: a manifest that asks for `keychain` is refused.

A picture an MCP tool returns (PNG or JPEG) reaches the model as an image, checked like
any other: at most 4 per result and 4 megapixels each. Each tool call carries the chat's
project folder in `_meta` under `dev.duskcode/project`, so a tool can keep to it.

### Talking to the network

The core's broker is an HTTP proxy at `DUSKCODE_PLUGIN_PROXY`, guarded by the token in
`DUSKCODE_PLUGIN_PROXY_TOKEN` (`Proxy-Authorization: Basic` of `dusk:<token>`; the
`HTTPS_PROXY` variables carry it already, so most HTTP libraries just work).

- A request in absolute form, `GET https://api.example.com/v1/items HTTP/1.1`, goes out
  when its origin is listed. `{{key:token}}` in the URL or a header value becomes the key
  on the way, only for an origin the key is for.
- `CONNECT api.example.com:443` opens a tunnel to a listed origin; nothing is put into
  a tunnel, so a key used this way must be `handed`.
- Addresses that are not public (private, loopback, link-local, metadata) are refused
  after the name is resolved, and the connection goes to the address that was checked.
  Redirects come back to the plugin; following one is a new request, checked again.
- Bodies are capped at 16 MiB and a request at 60 seconds. A key a server echoes back
  is hidden from the plugin.

### Where the rules are enforced

On macOS by the system's sandbox (Seatbelt): the keychain services are closed, network
is closed except the plugin's broker port, and only system programs, the plugin's own
and the ones it lists may start. On Linux by Landlock, which cannot keep a program to a
single port: a plugin that asks for network does not start there, and the keychain
(Secret Service, over D-Bus) cannot be closed. Landlock needs Linux 6.7 to close the
network at all; on an older kernel plugin programs do not start. Where a rule cannot be
enforced the program does not start.

A plugin that serves a contract (`provides:`) and asks for network is the exception on
Linux: each call runs in a user and network namespace of its own, whose only interface
is its own loopback, where the core's broker listens on port 3128. A seccomp filter lets
it open only IPv4, IPv6 and netlink sockets, so no Unix socket (the D-Bus session bus,
the Secret Service, docker, ssh or gpg agents, X11) and no vsock is reachable. So `gh`
must keep its token in `~/.config/gh/hosts.yml` (`gh auth login --insecure-storage`),
as the plugin's sign-in hint says. Where unprivileged user namespaces are off, or
restricted as Ubuntu's AppArmor does, such a plugin does not start and Settings, Plugins
says why on its row; there is no weaker fallback.

You see the permissions as plain lines before you switch the plugin on, and a plugin
update that asks for more stays off until you switch it on again.

## Interface parts

A plugin can add parts to the desktop window: an icon in the header, the rail or beside
the message box, or a page under Settings, which opens a panel, a bottom panel or a
window.

```yaml
ui:
  command: [./panel]          # the program that draws the parts; started when one opens
  parts:
    - id: simulator           # a-z, 0-9 and -, unique in the plugin
      title: iOS Simulator
      icon: icons/phone.svg   # SVG in the plugin's folder, at most 16 KiB
      place: header           # header | rail | composer | settings
      opens: right_panel      # right_panel | bottom_panel | window
      platforms: [macos]      # narrows the plugin's platforms; theirs when left out
```

The icon is drawn in the theme's colour; use `currentColor` for strokes. An SVG that
loads anything (scripts, images, external links or styles) is refused.

Many plugins stay calm: only parts you pin show their icon; the rest are under the
Plugins button, searchable and grouped by plugin. Panels open as tabs of one dock, and
a plugin's program runs only while one of its parts is open.

### The panel program

It reads one JSON object per line on stdin and writes one per line on stdout.

From the core:

| Message | Meaning |
| --- | --- |
| `{"type": "open", "part": "simulator"}` | a part opened; answer with its view |
| `{"type": "close", "part": "simulator"}` | it closed; stop its work |
| `{"type": "event", "part": "…", "widget": "boot", "event": {"kind": "click"}}` | a widget was used |

Events are `click`, `input` (`value`), `select` (`item`), `tap` (`x`, `y`),
`swipe` (`from`, `to`), and from a screen the desktop now sends: `touch` (`phase`
`down`/`move`/`up`, `x`, `y`) as the press goes, so a held press is a long press and a
drag follows the pointer; `scroll` (`x`, `y`, `dx`, `dy`) for the wheel; `key` (`key`,
such as `return`, `delete`, `left`, `select_all`, `undo`) and `text` (`text`) typed
while the screen has focus. Places are in the screen widget's own units.

To the core:

| Message | Meaning |
| --- | --- |
| `{"type": "view", "part": "…", "root": {…}}` | the part's whole view, replacing the last |
| `{"type": "frame", "part": "…", "widget": "screen", "jpeg": "<base64>"}` | a new picture for a screen |
| `{"type": "notice", "text": "…"}` | a short note in the window |

A view is a tree of the window's own widgets; a plugin never draws itself.

| Widget | Fields |
| --- | --- |
| `column`, `row` | `children` |
| `text` | `text`, `tone` (normal, muted, good, danger), `size` (title, body, small) |
| `button` | `id`, `label`, `primary`, `disabled` |
| `input` | `id`, `placeholder`, `value`; sends `input` on Enter |
| `list` | `id`, `items` (`id`, `title`, `detail`), `selected`, `menu` (a drop-down) |
| `screen` | `id`, `width`, `height`, `phone` (draw a phone around it); takes taps and swipes |
| `progress` | `text` |
| `divider` | |

Limits: 500 widgets, 8 levels, 4000 characters a text, 4 MiB a line, 3 MiB a frame. A
view past them is refused with a note, a longer line stops the program.

A minimal panel in Python:

```python
#!/usr/bin/env python3
import json, sys

def send(message):
    print(json.dumps(message), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if message["type"] == "open":
        send({"type": "view", "part": message["part"], "root": {"kind": "column", "children": [
            {"kind": "text", "text": "Hello from a plugin", "size": "title"},
            {"kind": "button", "id": "hi", "label": "Say hi", "primary": True}]}})
    elif message["type"] == "event" and message["widget"] == "hi":
        send({"type": "notice", "text": "Hi!"})
```

## Contracts

A plugin can serve a whole domain, such as the pull requests of a git host, instead of
adding tools or a panel. It says so under `provides:` in `plugin.yml`; the core then
starts its program to answer typed questions, and every client draws the answers with
its own widgets. This section is the normative text for a program in any language.

```yaml
permissions:
  exec: [gh, git]
  read: [~/.config/gh]           # gh's own sign-in, token included
  network: [https://api.github.com]
  project: read                  # the folder of the project a call is about, for that call
provides:
  command: [./bin/github]        # the contract program; ./ is the plugin's folder
  forge:                         # one field per contract; `forge` is the only one so far
    versions: [1]                # contract versions the program speaks
    hosts: [github.com]          # git remote hosts it serves
```

Checked when the plugin loads; an error shows in `/plugins`:

- `provides.command` is required when any contract is present; it is one program for all
  the plugin's contracts.
- A contract this duskcode does not know is refused: "this duskcode does not know the
  contract `x`; update duskcode".
- `versions` is a non-empty list of positive integers, at most 4.
- Forge `hosts`: 1 to 16 lowercase DNS names, each with a dot, at most 253 bytes, no
  wildcard, no IP literal, no port, not `localhost`, no duplicates.
- `permissions.project` (`none` or `read`) is refused in a plugin without `provides:`.
  `permissions.keychain` is refused in every plugin.
- A plugin with `provides:` may not also have `mcp`, `hooks`, `platform` or `ui`: "a
  plugin that provides a contract cannot also have mcp, hooks, a platform or ui (yet)".
- Before you switch the plugin on, `provides` is shown as a line: "serves pull requests
  and CI for github.com". An update that serves more hosts or more contracts than the
  installed plugin stays off until you switch it on again.

### One process per call

The core starts the program once per call, in the folder of the one project the call is
about, writes one line to its stdin, closes stdin, and reads its stdout. The program
answers and exits. Nothing is kept between calls, so an answer must not depend on state
the program kept. At most 4 calls run at once per plugin, and a call that has not
answered 45 seconds after it started is killed and counts as `unavailable`. A program
that exits without a result is `failed`, once what it left running has been killed. A
`result` line is at most 512 KiB; a larger one is `failed`.

When the call ends, the core kills the program's whole process group. A program that
leaves its group (with `setsid`, for one) is not killed then: it lives on, still
confined with that call's grants (the one project's folder and the plugin's own
permissions), until it exits.

Everything is one JSON object per line, UTF-8, a line at most 4 MiB. Unknown fields are
ignored, in both directions, within a version.

### The `call` line

From the core, the only line the program is sent:

```json
{"type":"call","contract":"forge","version":1,"op":"status","folder":"/Users/me/code/app","args":{}}
```

- `version` is the highest version both the manifest's `versions` and this duskcode
  speak. A program that cannot speak it answers `failed`.
- `op` is the operation; `args` is its arguments (below).
- `folder` is the project's canonical folder and also the program's working directory.
  It is the only project path the program is ever told.

### Answers: `result` and `log`

The program writes any number of `log` lines, then exactly one `result`, then exits.

| Line | Meaning |
| --- | --- |
| `{"type":"result","ok":{...}}` | The answer, in the shape of the operation. |
| `{"type":"result","error":{"kind":"signed_out","message":"...","hint":"gh auth login"}}` | A failure, by kind. `message` and `hint` may be left out. |
| `{"type":"log","text":"..."}` | A line for the debug log. Plain text; the core keeps at most 500 characters of it and at most 20 lines per call. |

Anything else before the result is a wire error and the call is `failed`. A `result`
with neither `ok` nor `error` is `failed`. The core stops reading at the first `result`:
lines after it are ignored, and a non-zero exit after it does not change it (the result
stands). A program that exits without a `result` is `failed`. A result that
does not parse into the shape of its operation is `failed`; the raw line is never shown.
What the program writes to stderr goes to the debug log, capped.

Error kinds:

| Kind | Use it when | The core shows |
| --- | --- | --- |
| `signed_out` | The tool the plugin uses is missing or not signed in. `hint` is what the user can type to fix it: plain, one line, at most 120 characters, such as `gh auth login`. It is drawn as text and never run. | The sign-in state of the forge, with the hint. |
| `not_found` | The account cannot see the repository, or the pull request does not exist. | For `pull`, nothing; for `status`, "{name} does not show this repository". |
| `unavailable` | The service did not answer: network down, rate limited, a timeout. | "{name} did not answer"; the core backs off and keeps the pull requests it had. |
| `refused` | The service refused an operation the user started, such as creating a pull request that exists already. `message` says why: plain, at most 200 characters are used. | The message, for `create_pull` only; elsewhere as `failed`. |
| `unsupported` | The operation is not known to this program. | As `failed`; the core does not ask again for this operation during the run. |
| `failed` | Anything else, and an unknown kind. | "{name} could not be read"; `message` goes to the debug log only. |

For `create_pull` the core never says "could not be read": the request may have gone
through before the failure. Any kind but `signed_out` with a `message` shows that
message (plain, at most 200 characters), so a program says what happened in its tool's
words and that the pull request may have been made. Without one, `unavailable` reads
"{name} did not answer in time; it may still have made the pull request, check {name}"
and anything else "{name} did not confirm the pull request; it may have been made,
check {name}"; so does a call that times out or an answer that is not a pull.

### What a call may reach

The program runs confined for this one call. It cannot widen this by any message.

| What | Granted |
| --- | --- |
| Programs | its own, and the ones in `permissions.exec`, found on `PATH` |
| Reads | everything outside your home folder but the other saved projects and the folders of temporary chats: so `/tmp`, `/opt`, mounted volumes and system files too. In home only: its own folder, its data folder (`DUSKCODE_PLUGIN_DATA`), `permissions.read`, git's own configuration (`~/.gitconfig`, `~/.config/git`), and this call's project folder when `permissions.project` is `read`. Never `~/.config/duskcode` or other plugins. |
| Writes | its data folder and the temporary folders only; never the project |
| Network | only through the core's broker (`HTTPS_PROXY`), to the origins in `permissions.network`, never to private addresses |
| Keychain | never on macOS: its files go with home and its services are closed. On Linux its files go with home; a call that may connect opens no Unix socket, so it cannot reach the Secret Service, but one that may not connect could, over D-Bus (Landlock does not reach Unix sockets). |
| Environment | the bare environment of every plugin program (see above). `GH_TOKEN`, `GITHUB_TOKEN` and every other variable of the core's environment never reach the program; a program sets what it needs (`GH_PROMPT_DISABLED=1`, `GH_PAGER=cat`, `NO_COLOR=1`, `GH_NO_UPDATE_NOTIFIER=1`) itself. |

On Linux a call runs in its own user and network namespace; where unprivileged user
namespaces are off, the plugin does not start and its row in Settings, Plugins says why.
Its Landlock rules handle every file right the kernel knows, truncation among them, so
nothing it may not write can be emptied either; where Landlock is older than Linux 6.2 a
plugin that serves a contract does not start, and its row says "this Linux cannot stop a
program from emptying files". A call refuses when `HOME` is `/`, empty or not a folder,
since home could not be hidden then.

### Versions

Each contract has an integer version, and the core sends the highest one the manifest
lists that it speaks. Within a version the core only adds optional fields to `args`,
adds error kinds (a program that does not know one reads it as `failed`), and adds
operations; a program answers an operation it does not know with
`{"type":"result","error":{"kind":"unsupported"}}`. A program therefore must ignore
unknown fields in `args`. Anything that would break a program that follows these rules is
a new version. The core speaks the newest version and the one before it for at least two
stable releases. This document carries the text of each version.

## Forge

A forge is a service that hosts git repositories with pull requests and CI: the contract
name is `forge`, version 1. A program for it serves the hosts named in
`provides.forge.hosts`; the core calls it only for a project whose `origin` is on one of
them. One call is about one repository.

Three operations. All times are Unix seconds or `null`; every list is capped by the core
after it reads it, whatever the program sends; a field left out reads as its empty value
(`""`, `0`, `false`, `[]`, `null`), except a pull's `number`, which is required. The
program maps its service's states onto the words below; the core never sees a vendor's.

### `status`

The pull requests that matter to a branch.

```json
{"type":"call","contract":"forge","version":1,"op":"status","folder":"/Users/me/code/app","args":{
  "host":"github.com","repository":"owner/repo","branch":"feature-x",
  "limits":{"branch_pulls":5,"open_pulls":10,"checks":500}}}
```

`repository` is as the remote names it (one to four path segments). `branch` is `null` on
a detached head. `limits` are what the core keeps; the program may stop reading there.
Answer:

```json
{"type":"result","ok":{
  "name":"GitHub",
  "pull_prefix":"https://github.com/owner/repo/pull/",
  "branch_pulls":[PULL],
  "open_pulls":[PULL]}}
```

`branch_pulls` are the branch's pull requests in any state, newest first. `open_pulls`
are the repository's open ones, most recently updated first.

### `pull`

One pull request by number, for a card in a chat.

```json
{"type":"call","contract":"forge","version":1,"op":"pull","folder":"/Users/me/code/app","args":{
  "host":"github.com","repository":"owner/repo","number":13}}
```

Answer `{"type":"result","ok":{"pull":PULL}}`; when there is none, `"pull":null` or the
error `not_found`.

### `create_pull`

A pull request from a branch the user already pushed.

```json
{"type":"call","contract":"forge","version":1,"op":"create_pull","folder":"/Users/me/code/app","args":{
  "host":"github.com","repository":"owner/repo","head":"feature-x","base":null,
  "title":"Fix the thing","body":"..."}}
```

`base: null` means the repository's default branch. Make the pull request the way the
service's own tool does; the GitHub plugin runs `gh pr create` and nothing more. Answer
`{"type":"result","ok":{"pull":{"number":12,"url":"https://github.com/owner/repo/pull/12"}}}`:
the number and the page the tool printed are enough, and the other fields of a `PULL`
may be left out. The core checks only the answer's shape (a `number` above 0 and a safe
`url`), not whose page it is, and the repository status shows the rest on its next
read. When the service refuses (no commits between the
branches, one exists already, the branch is not pushed), answer `refused` with the
service's message. The core has already checked the arguments: `head` and `base` are
valid branch names, `title` is a valid one-line message, and `body` is at most 20000
characters; the program need not check them again. `not_found` is not an answer of this
operation: the core reads it as `failed` (only `refused` carries a message to the user).

### A `PULL`

```json
{"number":12,"title":"Fix the thing","state":"open","branch":"feature-x",
 "from_fork":false,"author":"octocat","created":1763467641,"updated":1763470000,
 "added":10,"removed":2,"files":3,
 "url":"https://github.com/owner/repo/pull/12",
 "checks_url":"https://github.com/owner/repo/pull/12/checks",
 "checks":[{"name":"build","workflow":"CI","stage":"running",
            "url":"https://github.com/owner/repo/actions/runs/1/job/2",
            "started":1763467700,"finished":null}]}
```

- `state`: exactly `open`, `draft`, `merged` or `closed`; left out it reads as `open`.
  Any other word makes the whole answer `failed`, so map the service's states yourself.
- `stage` of a check: exactly `queued`, `running`, `passed`, `skipped` or `failed`; left
  out it reads as `queued`; any other word makes the whole answer `failed`. A check that
  was cancelled, timed out or needs action is `failed`; one the service skipped or found
  neutral is `skipped`.
- `branch` is the head branch; `from_fork` is true when the head lives in a fork.
- `files` may be `null`; `workflow` may be empty.
- `url` and `checks_url` are https links, `checks_url` being the page of all its checks.

### What the core checks

The core never trusts an answer. Text is made plain (control and bidi characters out) and
capped; a link is `https://` with only the characters `A-Za-z0-9-._~:/?#[]@!$&'()*+,;=%`
and at most 500 bytes, else it becomes empty (never repaired).

- `name`: one line, 1 to 40 characters; otherwise the plugin's name is shown.
- `pull_prefix` must start with `https://{host}/{repository path}/` (ASCII
  case-insensitive), end with `/`, and be at most 300 bytes of the safe link characters.
  Otherwise the whole answer is `failed`: without a trusted prefix no pull can be checked.
  Every service has one: `/owner/repo/pull/`, `/group/proj/-/merge_requests/`,
  `/owner/repo/pulls/`, `/ws/repo/pull-requests/`.
- A pull's `url` must equal `pull_prefix` plus its `number` (case-insensitive), or the
  pull is dropped. Its `number` must be above 0. The pull `create_pull` answers is held to
  its shape only (see `create_pull`).
- `checks_url` must start with `https://{host}/`, else it becomes empty. A check's `url`
  is any safe https link, else empty.
- Title at most 300 characters, author 100, branch 250.
- `branch_pulls` keeps only pulls that are not `from_fork` and whose `branch` is the one
  asked, at most 5. `open_pulls` keeps at most 10, leaving out any number among the
  branch's.
- Checks: at most 500 are counted and at most 50 listed, ordered failed, running,
  queued, passed, skipped; the roll-up (counts and the worst state) is the core's.

## The store

The Workshop under Plugins lists what the store and its mirrors offer and installs a
plugin with a click, after showing what it may do. The official list is at
`https://drobov1tya.github.io/duskcode-workshop/index.json`; when that address cannot be
reached, the old `https://drobov1tya.github.io/duskcode-plugins/index.json` is tried
(it stopped answering when the repository was renamed on 2026-11-18).
The core installs a plugin only when:

- the list is signed (Ed25519) by its source's key: the key built into duskcode for the
  official list, the key written in `config.yml` for a mirror;
- the list's `serial` is no older than one accepted from that source before, and it has
  not `expires`d;
- the package for this system and processor has the size and SHA-256 the list names;
- it unpacks to plain files and folders inside its own folder: no links, no `..`, no
  absolute paths, at most 4000 files and 256 MiB;
- its `plugin.yml` asks for exactly the permissions, systems and clients the list
  showed you.

The store updates and removes only what it installed (`store.json` in the config folder
records each plugin with its source); a folder you made yourself is never replaced. A
plugin is updated only from the source that installed it, by its name, address and key,
so two sources offering the same name never swap one for the other; a mirror that
changed its address or key is not updated from until you reinstall. "Update all" updates every plugin with a newer
version (semver) that asks for nothing more than the version here; one that asks for
more is left for you to update by hand. A browser can look at the store but not install
or update from it: a plugin runs programs on this machine.

Single skills are offered on the Skills page; plugins, packs and agents under Plugins;
connectors, and the plugins that bring an MCP server, under Connectors.
A client asks the core for the kinds its page shows (`StoreRequest::List` with `kinds`),
so the split is the core's. Each page opens on Discover while nothing is installed there.

### Connectors

A connector listing (`kind: connector`) is a service's MCP server with nothing to
download: its `server` says the `https` address it is at, or `npx`/`uvx` with a package
pinned to one exact version, how it signs in (`none`, `oauth`, or a `key` the user puts
in), the names of the headers or environment variables that key goes in (never a value),
the service's page about it, who runs it and what it reaches. Customize, Connectors,
Discover offers them with Add. Adding one writes the server into `config.yml` through the
same path as the MCP settings, after a window that shows all of that; a server with a key
is written switched off with those fields empty, and OAuth goes through the existing MCP
sign-in. `store.json` records which connectors the store added, apart from the plugins,
so an older duskcode reads it as before; Remove takes the server out unless you changed
it since. A server of yours with the same name is never replaced.

The address is `https` on the default port, on a named host written in plain ASCII (no
punycode label), as it is written to the config and shown; names only a private network
answers (`.local`, `.lan`, `.corp`, `.internal`, `.home.arpa`, `localhost` and the like)
are refused, and the name must resolve to public addresses when the connector is added.
A package is `npx -y name@MAJOR.MINOR.PATCH` with an npm registry name, or `uvx
name===MAJOR.MINOR.PATCH` (`===`, so no padded or local version matches) with a
normalised PyPI name: never a range, tag, folder,
tarball or another protocol. A key goes in a header or variable the user names, never
one that changes what runs (`PATH`, `NODE_OPTIONS`, `npm_config_*`, `PYTHON*`, `LD_*`,
`DYLD_*` and the like) or one the transport sets. A connector never takes the name of a
server a plugin here brings, and from any source but the official one not a name close
to a server already set up. A local connector runs a program as you, so only an Official
or Verified listing may bring one; a source without a key may offer only remote ones,
and its confirmation says that nobody vouches for it and names the host its sign-in and
keys go to. The MCP list shows which source added a server and its tier; a server the
user removes, renames or points elsewhere in the settings is theirs from then on. A
duskcode before connectors leaves such entries out of a list and reads the rest.

A local connector may instead be a program from a GitHub release, listed as `kind:
release-connector` (a duskcode before it leaves that kind out, where it would refuse the
whole list over a `connector` naming a `release`); it is read as a connector, a
`release-connector` must name a `release` and a `connector` must not. `server.release` names
the `repository` (`owner/name` on github.com), one exact `tag` (never `latest`) and, per
Rust target triple, the archive's `file`, its `sha256`, the `binary`'s path inside it and
optionally its `size`; `args`, `auth` and `secrets` are as for any local one. Only the
official list may bring it, under its Official badge. Adding it picks the archive for
this computer (refused when there is none), downloads it from
`github.com/<repository>/releases/download/<tag>/<file>` with the store's client
(redirects only to GitHub's asset hosts, capped at 128 MB), checks its size and SHA-256
before unpacking, takes out only the program under the same rules as a package (no links,
no path out of its folder, size and entry caps), puts it in
`<data>/connectors/<name>/<tag>/`, executable, with the macOS quarantine mark taken off
it alone, and writes the server to start it by that absolute path. The confirmation
names the repository, tag, archive, the start of its SHA-256 and its size, and that it
runs with your rights. Removing the connector removes its folder; an update is a new tag
in a new folder, and the old one goes once the new one is added. Only `.tar.gz` archives
are unpacked.

### Contents

A listing's page shows its files on a Contents tab, fetched only when the tab opens. The
core lists the copy installed here, edits included, or else downloads the package the
signed list names, checks its size and SHA-256, and reads it in memory under the unpack
rules below; nothing of it is written out, run or read by the model. The last three
listings opened are kept. A file is answered only by a path of that listing, never joined
from what a client sends, only as UTF-8 text (programs and other files show their size),
at most 256 KiB, with control and bidi characters shown as escapes. Links in an installed
folder are neither followed nor listed.

### Tiers

Every listing carries the tier of the key that checked its list; a field of the list
can only lower it, never raise it:

| Tier | Where it comes from |
| --- | --- |
| Official | the official list, signed by the key built in, for the work of duskcode's maintainers |
| Verified | the official list for somebody else's work (`badge: verified`, or an `author` who is not a maintainer), or a mirror you added whose address and key the official list names under `verified` |
| Community | any other mirror you added |
| Unverified | a source you added without a key: nobody signed its list, whatever any list says |
| Local | a plugin in the plugins folder that no source installed; no key vouches for it |

An install names the tier you were shown and is refused when the listing earns another
one now. A mirror's `badge` is never read. A listing whose name another source also offers is marked, since a mirror may
copy a name to pass for another source's plugin.

The list comes sorted in that order, then by name; a client may ask for the newest or
the recently updated first, or for one category. Sorting is the core's, so every client
shows the same. Nothing is sorted or picked by stars: a mirror writes its own count.

### Mirrors

A mirror is added in Settings, Workshop, or in `config.yml`; its key, when given, is
pinned, so a mirror signed by another key is refused, not asked about. In Settings you
type the list's https address, or a GitHub repository's (`https://github.com/owner/name`,
read as `https://owner.github.io/name/index.json`), and its key; Check reads the list
without saving it (`StoreRequest::Probe`), Add reads it again in the core and saves it
under a name taken from the address (`AddSource`), and each row shows whether its list
was read or why it was refused (`Sources`). A browser may list them but not probe, add
or remove one. Everything a repository offers is Community, whatever its list says:

```yaml
workshop:
  sources:
    - name: friends                                   # a-z, 0-9 and -; not official or local
      url: https://friends.example.com/index.json     # https only
      key: BASE64-OF-ITS-ED25519-PUBLIC-KEY
    - name: skill-hub                                 # no key: Unverified
      url: https://hub.example.com/index.json
```

At most 16 mirrors and 2000 entries per list; a list or a picture that takes over 20 s
is given up on (a package 120 s). All sources are read at once; one that fails is reported beside the
others, which are listed all the same.

The store fetches over https, and only from public addresses: a name that resolves to
this machine, a private or link-local network, carrier NAT, multicast or a cloud's
metadata service is refused. Lists, covers and screenshots follow no redirect. A package may follow up
to three https redirects to the host it was asked from or to GitHub's asset hosts
(`github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`),
as release downloads need; its SHA-256 is checked either way.

### Sources without a key

A source added without a key (left empty in Settings, or no `key:` in `config.yml`) is
read unsigned: its `.sig` is not fetched, and every other check of a list stands (https
only, public addresses, no redirects, size and entry caps, format, expiry, names, paths,
per-file SHA-256). Everything it offers is **Unverified**, even when the official list
names its address: only a key vouches. What it may bring is held tighter
(`plugins/store/guard.rs`):

- only skills, packs and agents whose files are pinned upstream (a GitHub commit and the
  SHA-256 of every file, UTF-8 text only); never a plugin (a program, or a connector that
  runs one) and never a package. What is left out is said on the source's row
  ("2 plugins ignored: an unverified source cannot bring programs");
- no cover, screenshot or star count is read from its list; the name's gradient stands in;
- its serial is not held to: whoever serves an unsigned list can write any serial, so the
  rule would protect nothing and could lock you out. The commit and file hashes the
  record keeps of what was installed are what holds;
- its names never pass for a trusted or installed one: a name that is, or is close to
  (dashes and case aside: one edit for 5 letters or more, two for 8 or more, a swap of
  two letters counting as one, or a name of 8 or more with words added), a name an
  Official or Verified listing offers, the official list's names as last read, anything
  installed from another source, a built-in skill, the `duskcode` pack or a folder under
  `skills/` or `agents/` is shown and refused as taken;
- a listing shows its source and the host of its list, and its `author` as that source's
  claim; its origin is the pinned repository, not what the list says.

Installing or updating one first asks the core for a review (`StoreRequest::Review`): the
files it writes, fetched and checked, with what an update adds, changes and removes
against the copy on disk and the diff of its `SKILL.md` or `AGENT.md`, the scripts it
carries, and for an agent the tools it gets (every tool when its `tools:` names none). The
confirmation says plainly that nobody vouches for it. The install names the review's
`package_sha256`, and the core refuses it when the files are no longer those. "Update all"
never updates an Unverified item: it is left for you, with why.

What an Unverified skill or agent then asks of the agent is your call. `load_skill` gives a
skill's text, and an agent's prompt carries its text, between a line saying it comes from
an unverified source and is data from a third party, not instructions from you, and a line
closing it; a line of the text that would pass for either is taken out. The skill list and
`ask_agent`'s list mark it with its source. A skill's `links:` lead only to skills of its
own folder, never to built-in skills such as `skill-evolve` or to your own: those are the
only skills `load_skill` offers after it. Its text may still name another skill, which the
model may load as it may any skill listed; the marker is what tells it whose words those
are. When the store's record cannot be read, everything the store installed counts as
from an unknown, unverified source. Removing such an item keeps the files you edited, and they stay
marked as that source's, not your own, while their folder is there. No source but the official one may bring an agent
named as, or close to, an agent duskcode brings (`general` and the shipped ones).
Its scripts are plain text, written readable and writable by you only, and run under the
permission mode you chose, like any command.

#### Adding a source from a chat

When you ask the agent to connect a catalogue, it may find its list and call
`add_workshop_source { name, url, key? }`. That only proposes it: you are asked every
time, in every mode, full access included, with the name, the address, whether it is
keyed or Unverified and what an Unverified source may bring. Only a client that could add
the source itself may answer (the desktop app or the terminal; not a browser, not a chat
shared through a scope bound to it, not a chat platform). The yes adds it the way
Settings does, reading the list again, under a name the core gives it from the address
(the agent's name is kept only when it is made of the address's own words and claims no
trust, such as `official`), and the tool answers with how many listings it
read. The address is refused before you are asked when it is not https or not public. In a chat nobody who may answer is
in (a scheduled run, a headless `duskcode run`, a chat an API token started) the call is
refused at once, so a run never waits for an answer that cannot come.

### index.json

Format 2 (format 1, the same without the new fields and `verified`, is still read):

```json
{"format": 2, "serial": 7, "expires": "2027-01-01T00:00:00Z",
 "plugins": [{
  "name": "telegram", "version": "1.0.0", "kind": "plugin", "description": "...",
  "category": "chat", "tags": ["bot", "voice"],
  "cover": {"path": "covers/telegram.webp", "sha256": "...", "size": 40213},
  "screenshots": [{"path": "screenshots/telegram/1-chat.webp", "sha256": "...",
                   "size": 182044, "caption": "A thread answered from Telegram"}],
  "stars": 42, "created": "2026-01-02T00:00:00Z", "updated": "2026-03-04T00:00:00Z",
  "platforms": ["macos"], "clients": ["gui", "tui"],
  "permissions": {"network": ["https://api.telegram.org"]},
  "homepage": "https://...",
  "downloads": [{"os": "macos", "arch": "aarch64", "url": "https://...tar.gz",
                 "sha256": "...", "size": 123456}]}],
 "verified": [{"name": "friends", "url": "https://friends.example.com/index.json",
               "key": "..."}]}
```

- `kind` is `plugin`, `skill`, `pack` (with `skills: [names]`) or `agent`. An entry of a
  kind this duskcode does not know is left out alone.
- A skill, pack or agent is plain files, so it has one package with `"os": "any",
  "arch": "any"`; a plugin runs programs, so a plugin package for `any` is refused.
- `category` and `tags` (up to 8) are `a-z`, `0-9` and `-`.
- `cover` is a `.png` or `.webp` of at most 512 KiB, by a path under the list's folder;
  the core fetches it, checks its size, SHA-256 and first bytes, and keeps it in memory.
  Before anything decodes it, the size its header declares must be within 2400x1260
  (3200x2000 for a screenshot) and a WebP must not be animated.
- `screenshots` (up to 8) are pictures of the item at work, each checked like a cover but
  up to 1 MiB, with an optional `caption`: one line of at most 120 characters, without
  control characters or bidi overrides. A client gets their count and captions in the
  listing and asks for each by its place (`StoreRequest::Screenshot`); the core fetches
  only what the last list read named, from a source that is still the one configured.
  Covers and screenshots share a 32 MiB in-memory cache. A bad cover or screenshot
  refuses the whole list, as any bad field does.
- `stars` is the GitHub star count of the repository `homepage` names, counted by the
  publisher when signing; duskcode never calls GitHub. It is a repository's number, not
  a rating, so duskcode shows it only for a mirror you added, and only when `homepage`
  is a `https://github.com/owner/name` page, as "122 GitHub stars of owner/name" on the
  listing's page. The official list's and a verified mirror's `stars` are dropped when
  read: they were the store's own or a vendor's whole repository's.
- `created` and `updated` are RFC 3339.
- `verified` is believed only in the official list.

Format 3 adds who made each entry, and every entry must give all of it:

```json
  "author": "someone", "license": "MIT", "source": "https://github.com/someone/x",
  "categories": ["coding", "review"], "badge": "verified"
```

- `author` and `license` (SPDX) are one plain line of at most 64 characters, without
  control characters or bidi overrides; `source` is an https address of at most 200.
  The listing carries `source` as `origin`, since `source` names the source there.
- `categories` are 1 to 3 different ones of `coding`, `review`, `testing`, `security`,
  `research`, `writing`, `data`, `devops`, `design`, `productivity`, `devices`,
  `communication` (`dusk_protocol::plugin::CATEGORIES`).
- `badge` is `official` or `verified`. In the official list, `verified`, or an `author`
  who is not a maintainer, makes the listing Verified; elsewhere it is ignored, and a
  Community listing's `author` is drawn as the mirror's claim.
- A format 2 list with any of these fields is refused, as is a format 4 list: an older
  duskcode answers a newer list with "update it".

A format 3 plugin entry also says what it runs and adds, and its `plugin.yml` must say
exactly the same or the package is refused:

```json
  "mcp": [{"name": "search", "command": "./search-server", "args": ["--fast"]},
          {"name": "docs", "url": "https://docs.example.com/mcp"}],
  "programs": ["./bin/bot"],
  "skills": ["triage"]
```

- `mcp`: up to 16 servers, each a program it starts (`command`, `args`) or an `https`
  address, as in its manifest. `programs`: the command lines of its platform, panel and
  hooks, up to 8, sorted. `skills`: the folders under its `skills/`.
- Each program and server is a line of what the user agrees to, before the permission
  lines, so an install that was not agreed to with them is refused and nothing of the
  plugin is placed or started. An update that adds one is left for the user, as one that
  asks for more permissions is. A list before format 3 cannot name them, and its plugins
  install as before.
- The listing shows the servers on a Connectors tab: "Not added", "Added" while the plugin
  is on, or "Yours is used" when a connector of the user's `config.yml` has that name. A
  plugin's servers are among the connectors only while it is on and go with it; a
  connector the user configured is never replaced.

An unknown field anywhere refuses the list. `index.json.sig` beside it holds the base64
of the signature of the file's exact bytes. `DUSKCODE_STORE_URL` points duskcode at
another official list, such as a fork's; it is still checked against the built-in key.

### Skills, packs and agents

Their packages unpack under the same rules as a plugin's and land where the core reads
its own skills and agents:

| Kind | Package | Installed to |
| --- | --- | --- |
| `skill` | `<skill>/SKILL.md` and its other files | `skills/<skill>/` |
| `pack` | `pack.yml` and `<skill>/...` for each skill | `skills/<pack>/<skill>/` |
| `agent` | `<agent>/AGENT.md`, or `pack.yml` and `<agent>/...` | `agents/<agent>/` or `agents/<pack>/<agent>/` |

```yaml
# pack.yml; an unknown field refuses the pack
name: duskcode-engineering   # a-z, 0-9 and -; the same as the listing's
title: Engineering
description: Ways of working on code.
vendor: duskcode
kind: skills                 # skills | agents
items: [spike, systematic-debugging]   # one folder each, a-z, 0-9 and -
license: MIT
upstream: https://github.com/...       # optional
```

A package that holds anything else, names other items than the listing or asks for
permissions is refused, and so is one whose folder is already there and not from the
store. A skill in a pack is named by its path, such as `duskcode-engineering/spike`, so
links and `skills:` of agents in a pack use those names. The store records what it wrote
by hash: an update or a removal never replaces or deletes a file you edited since.

Skills are switched on and off by pack too. The built-in skills form the pack
`duskcode`, and every folder under `skills/` that holds skills is a pack. A pack switched
off hides all its skills; in a pack that is on, a skill switched off hides just it:

```yaml
skill_packs:
  openai: false
skills:
  duskcode-engineering/spike: false
```

### From upstream

A format 3 entry of kind `skill`, `pack` or `agent` may name, instead of `downloads`, the files
of a commit of its author's GitHub repository:

```json
"upstream": {
  "repo": "owner/name", "commit": "<40 hex>",
  "files": [{"path": "skills/x/SKILL.md", "to": "x/SKILL.md", "sha256": "...", "size": 1234}]
}
```

"Install from source" fetches each file from
`https://raw.githubusercontent.com/<repo>/<commit>/<path>`, without redirects, and checks
all of them before writing any: a file of another size or hash, a file not named, a
missing one or one that is not UTF-8 text refuses the install. At most 200 files of 4 MiB
each and 32 MiB in all (an item over that cannot be installed; the rest of its list
reads as before); `path` and `to` are plain relative paths. A pack's `pack.yml` is written by the core
from the entry. The record keeps the item's author, license and the commit's address.
Contents does not show such an item before it is installed (there is no package).

## Plugins in any language

The core never loads a plugin's code. Everything a plugin runs is a separate process
the core starts: MCP servers over stdio, hooks, a platform and a panel program, which
trade JSON with the core (one object per line, or one object on stdin for a hook). So a
plugin can be written in any language. In the store it ships either as packages per
system and processor (`os`, `arch` in `downloads`), for a compiled program, or as one
package for all, for a script run by an interpreter already on the machine. A `runtime` field that names such an interpreter is planned, not read yet.

## Plugins of the store

Telegram and the rest live in their own repository,
[duskcode-workshop](https://github.com/DroBov1Tya/duskcode-workshop), and install from
Plugins, Workshop. duskcode ships no plugin in its binary.
