---
name: duskcode
description: "Extending duskcode itself: add an agent, a skill or an MCP server. Use when asked to add or change dusk's agents, skills or MCP servers."
source: "dusk_code"
author: "Neil Ermak"
license: MIT
tags: [dusk, setup]
---
# Extending duskcode

This skill's folder is `<config>/skills/duskcode`, so the dusk config folder `<config>` is
two levels above it. Everything below lives there.

| Add | Where | Applies |
|---|---|---|
| skill | `<config>/skills/<name>/SKILL.md` | at once |
| agent | `<config>/agents/<name>/AGENT.md` | within a minute |
| MCP server | `mcp:` in `<config>/config.yml` | when the user saves it |

Names: letters, digits, `-` and `_`. A folder inside a folder gives `group/name`.

## Skill

Instructions an agent loads when its description matches the task.

```markdown
---
description: What it does and when to use it, one line. Required.
tags: [git, release]   # optional, shown in /settings
links: [other-skill]   # optional, skills to load after this one
entry: false           # optional; hides it from the first list, reached by links only
---
Short imperative steps, each checked in practice. Scripts and references go in the
same folder and are named by their path in it.
```

## Agent

An expert with its own thread. The text after the front matter is its system prompt.

```markdown
---
description: Its expertise, one line; the chat reads it to decide whom to ask. Required.
tools: [read_file, run_command, load_skill]   # optional; every tool when left out
links: [tester]   # optional; agents it may ask
entry: false      # optional; then only agents that link to it can ask it
---
You are ... How you work: ... What you return: ...
```

Tools: `read_file`, `write_file`, `edit_file`, `delete_file`, `run_command`,
`read_history`, `tasks`, `load_skill`, `ask_agent`, and MCP tools as `<server>__<tool>`.

## MCP server

Tools cannot open `config.yml`: it holds API keys. Write the server's YAML and ask the
user to add it in `/settings`, `mcp`, `n`: paste, then Ctrl+S. It starts at once.

Local, started by dusk:

```yaml
github:
  command: npx
  args: [-y, "@modelcontextprotocol/server-github"]
  env:
    GITHUB_PERSONAL_ACCESS_TOKEN: <token>
```

Remote:

```yaml
docs:
  url: https://mcp.example.com/mcp
  headers:
    Authorization: Bearer <token>
```

- Name: up to 24 of `a-z`, `0-9`, `-`.
- Either `command` with optional `args` and `env`, or `url` (HTTPS, or HTTP to
  `127.0.0.1`) with optional `headers`. Never both.
- `enabled: false` keeps a server without starting it.
- No other fields: an unknown one makes the whole config invalid.
- Leave `<token>` for the user; never ask for a secret in the chat.

If the user edits `config.yml` by hand: change only `mcp:`. Leave `providers` (API keys),
`model`, `limits`, `thread` and the `tools`, `skills`, `agents` switches alone; dusk
rewrites the file and drops comments.

`<config>/MEMORY.md` is what you remember about the user. Change it only with the
`remember` tool, never by hand; the user switches it off in `/settings` -> tools.
