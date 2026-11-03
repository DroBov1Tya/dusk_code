---
name: webapp-testing
description: Toolkit for interacting with and testing local web applications using Playwright. Supports verifying frontend functionality, debugging UI behavior, capturing browser screenshots, and viewing browser logs.
source: "Anthropic"
author: "Anthropic"
license: Apache-2.0
upstream: https://github.com/anthropics/skills/tree/main/skills/webapp-testing
changes: "dusk_code: Markdown-only edition; replaced executable server helper and examples with direct testing guidance"
---

# Web Application Testing

Use an available browser tool or the project's installed Playwright dependency to
verify the requested local web application behavior. No executable helpers, server
manager, or Python examples are bundled with this skill.

1. Read the project's startup and test instructions. Identify existing listeners
   before starting another server; do not stop an unrelated process.
2. Start required services with the project's documented commands in tracked terminal
   sessions. Check readiness at the intended local URL, and record which sessions
   this test owns. If startup fails, capture the relevant error and stop dependent work.
3. Open the page, wait for a meaningful application-ready element, and inspect the
   rendered DOM or browser snapshot. Network idleness alone is insufficient for apps
   with polling or persistent connections.
4. Choose selectors from observed roles, labels, or stable test identifiers. Exercise
   the requested journey and assert its visible result, including error/empty states
   relevant to the request. Reinspect after navigation and substantial UI updates.
5. Capture screenshots and console errors where useful. Save evidence and any task-
   specific automation outside the skill folder. Close owned browser sessions and
   stop only services started by this test, including after a failure.
6. Report the URL, environment, actions, observed outcome, and unchecked scenarios.
   A screenshot or successful startup alone does not prove the user journey works.
