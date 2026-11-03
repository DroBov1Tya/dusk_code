---
name: "playwright"
description: "Use when the task requires automating a real browser from the terminal (navigation, form filling, snapshots, screenshots, data extraction, UI-flow debugging) via `playwright-cli`."
source: "OpenAI"
author: "OpenAI"
license: Apache-2.0
upstream: https://github.com/openai/skills/tree/main/skills/.curated/playwright
changes: "dusk_code: Markdown-only edition; removed wrapper, use installed playwright-cli"
---


# Playwright CLI Skill

Drive a real browser from the terminal using `playwright-cli`. Use the project-installed `playwright-cli`; verify its availability and `--help` before use. No wrapper is bundled.
Treat this skill as CLI-first automation. Do not pivot to `@playwright/test` unless the user explicitly asks for test files.

## Prerequisite check

Check `command -v playwright-cli` and inspect `playwright-cli --help` before use.
If the CLI is unavailable, report the dependency and propose a project-appropriate
installation. Do not install packages or assume a bundled wrapper is present.

## Quick start

With the installed CLI:

```bash
playwright-cli open https://playwright.dev --headed
playwright-cli snapshot
playwright-cli click e15
playwright-cli type "Playwright"
playwright-cli press Enter
playwright-cli screenshot
```

If the user prefers a global install, this is also valid:

```bash
npm install -g @playwright/cli@latest
playwright-cli --help
```

## Core workflow

1. Open the page.
2. Snapshot to get stable element refs.
3. Interact using refs from the latest snapshot.
4. Re-snapshot after navigation or significant DOM changes.
5. Capture artifacts (screenshot, pdf, traces) when useful.

Minimal loop:

```bash
playwright-cli open https://example.com
playwright-cli snapshot
playwright-cli click e3
playwright-cli snapshot
```

## When to snapshot again

Snapshot again after:

- navigation
- clicking elements that change the UI substantially
- opening/closing modals or menus
- tab switches

Refs can go stale. When a command fails due to a missing ref, snapshot again.

## Recommended patterns

### Form fill and submit

```bash
playwright-cli open https://example.com/form
playwright-cli snapshot
playwright-cli fill e1 "user@example.com"
playwright-cli fill e2 "password123"
playwright-cli click e3
playwright-cli snapshot
```

### Debug a UI flow with traces

```bash
playwright-cli open https://example.com --headed
playwright-cli tracing-start
# ...interactions...
playwright-cli tracing-stop
```

### Multi-tab work

```bash
playwright-cli tab-new https://example.com
playwright-cli tab-list
playwright-cli tab-select 0
playwright-cli snapshot
```

## References

Open only what you need:

- CLI command reference: `references/cli.md`
- Practical workflows and troubleshooting: `references/workflows.md`

## Guardrails

- Always snapshot before referencing element ids like `e12`.
- Re-snapshot when refs seem stale.
- Prefer explicit commands over `eval` and `run-code` unless needed.
- When you do not have a fresh snapshot, use placeholder refs like `eX` and say why; do not bypass refs with `run-code`.
- Use `--headed` when a visual check will help.
- When capturing artifacts in this repo, use `output/playwright/` and avoid introducing new top-level artifact folders.
- Default to CLI commands and workflows, not Playwright test specs.
