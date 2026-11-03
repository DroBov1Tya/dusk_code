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

## Built-in plugins

duskcode ships its own plugins the same way, so they follow each update:

| Plugin | What it does | Settings |
| --- | --- | --- |
| `telegram` | Talk to duskcode from Telegram: messages, answers, questions and approvals | `token`, from @BotFather |

To use the Telegram bot, add its token under `plugins: telegram: settings:` in
`config.yml`, switch it on in `/plugins`, and send the bot the `/start` code that
`/plugins` shows.
