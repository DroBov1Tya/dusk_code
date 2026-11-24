---
name: duskcode
description: "Where dusk's own agents, skills, MCP servers and memory live and the front matter or YAML each takes. Use when asked to add, change or configure one of them in dusk itself; not for the user's project code."
source: "Duskcode"
author: "Duskcode"
license: MIT
tags: [dusk, setup]
---
# Extending duskcode

This skill's folder is `<rules>/skills/duskcode`, so the rules folder `<rules>` is two
levels above it, and the dusk config folder `<config>` one above that.

| Add | Where | Applies |
|---|---|---|
| skill | `<rules>/skills/<name>/SKILL.md` | at once |
| agent | `<rules>/agents/<name>/AGENT.md` | within a minute |
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
Short imperative steps, each checked in practice.
```

Only Markdown files belong in the skill folder: put longer instructions in linked
`references/<topic>.md` files and keep helpers and generated files outside it. To write
or revise a skill well, load `skill-creator`.

## Agent

An expert with its own thread. The text after the front matter is its system prompt.

```markdown
---
description: Its expertise, one line; the chat reads it to decide whom to ask. Required.
tools: [read_file, run_command, load_skill]   # optional; every tool when left out
links: [tester]   # optional; agents it may ask
skills: [token-thrift]            # optional; skills its prompt points it to
entry: false      # optional; then only agents that link to it can ask it
---
You are ... How you work: ...
```

Dusk adds the common rules to every agent's prompt: how to treat what it reads, the format
of its final report, and how to delegate. Write only what is particular to this agent.

Tools: `read_file`, `search_files`, `changes`, `project_graph`, `recall`, `run_command`,
`jobs`, `read_history`, `tasks`, `load_skill`, `ask_agent`, and MCP tools as
`<server>__<tool>`. Give an agent only the tools its work needs. A list leaves out MCP
tools it does not name. Files change only through `run_command`: an agent without it
can read but not write.

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
- No other fields but `icon`: an unknown one makes the whole config invalid.
- Leave `<token>` for the user; never ask for a secret in the chat.

If the user edits `config.yml` by hand: change only `mcp:`. Leave `providers` (API keys),
`model`, `mode`, `effort`, `limits`, `thread` and the `tools`, `modes`, `skills`, `agents`
switches alone; dusk rewrites the file and drops comments.

## Memory

`<rules>/MEMORY.md` is what you remember about the user, and
`~/Documents/duskcode/projects/<folder>/MEMORY.md` what you remember about a project.
Change them only with the `remember` and `remember_project` tools, never by hand; the
user switches them in `/settings` -> personalize.
