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
  to the origins the manifest lists, and never to private or local addresses.
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
```

A picture an MCP tool returns (PNG or JPEG) reaches the model as an image, checked like
any other: at most 4 per result and 4 megapixels each.

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

Events are `click`, `input` (`value`), `select` (`item`), `tap` (`x`, `y`) and
`swipe` (`from`, `to`), places in the screen widget's own units.

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

### Tiers

Every listing carries the tier of the key that checked its list, never a field of the
list itself:

| Tier | Where it comes from |
| --- | --- |
| Official | the official list, signed by the key built in |
| Verified | a mirror you added whose address and key the official list names under `verified` |
| Community | any other mirror you added |
| Local | a plugin in the plugins folder that no source installed; no key vouches for it |

An install names the tier you were shown and is refused when the source earns another
one now. A listing whose name another source also offers is marked, since a mirror may
copy a name to pass for another source's plugin.

The list comes sorted in that order, then by name; a client may ask for the newest,
the recently updated or the most starred first, or for one category. Sorting is the
core's, so every client shows the same.

### Mirrors

A mirror is added in `config.yml`; its key is required and pinned, so a mirror signed
by another key is refused, not asked about:

```yaml
workshop:
  sources:
    - name: friends                                   # a-z, 0-9 and -; not official or local
      url: https://friends.example.com/index.json     # https only
      key: BASE64-OF-ITS-ED25519-PUBLIC-KEY
```

At most 16 mirrors. All sources are read at once; one that fails is reported beside the
others, which are listed all the same.

The store fetches over https, and only from public addresses: a name that resolves to
this machine, a private or link-local network, carrier NAT, multicast or a cloud's
metadata service is refused. Lists and covers follow no redirect. A package may follow up
to three https redirects to the host it was asked from or to GitHub's asset hosts
(`github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`),
as release downloads need; its SHA-256 is checked either way.

### index.json

Format 2 (format 1, the same without the new fields and `verified`, is still read):

```json
{"format": 2, "serial": 7, "expires": "2027-01-01T00:00:00Z",
 "plugins": [{
  "name": "telegram", "version": "1.0.0", "kind": "plugin", "description": "...",
  "category": "chat", "tags": ["bot", "voice"],
  "cover": {"path": "covers/telegram.webp", "sha256": "...", "size": 40213},
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
  the core fetches it, checks its size, SHA-256 and first bytes, and keeps the last 64.
- `stars` is counted by the publisher when signing; duskcode never calls GitHub.
- `created` and `updated` are RFC 3339.
- `verified` is believed only in the official list.

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
