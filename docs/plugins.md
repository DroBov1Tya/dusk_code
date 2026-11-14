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
  exec: [xcrun, axe]                     # programs it may start besides its own
  read: [~/Library/Developer]            # folders outside its own it may read
  keys:
    - name: token
      origins: [https://api.example.com]
      handed: false                      # true: the plugin reads the key itself
```

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

## Built-in plugins

duskcode ships its own plugins the same way, so they follow each update:

| Plugin | What it does | Settings |
| --- | --- | --- |
| `telegram` | Talk to duskcode from Telegram: messages, answers, questions and approvals | `token`, from @BotFather |

To use the Telegram bot, add its token under `plugins: telegram: settings:` in
`config.yml`, switch it on in `/plugins`, and send the bot the `/start` code that
`/plugins` shows.
