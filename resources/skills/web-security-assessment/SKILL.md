---
name: web-security-assessment
description: Discover and assess web attack surfaces with bounded crawling, targeted fuzzing and browser/server evidence.
source: Dusk Code
tags: [security, assessment]
---

# Web assessment

Use security-assessment for scope, stage gates, bounds and evidence handoffs.
Use OWASP WSTG as a coverage taxonomy, selecting relevant tests rather than claiming
full coverage. Record the consulted version/reference when using specific guidance.

Start with origins, virtual hosts, technologies, auth states and source routes.
Reconcile browser navigation, proxy traffic, forms, JavaScript routes and declared
APIs. Crawl allowed origins with separate anonymous/authenticated coverage; record
methods, inputs, state-changing actions and unvisited paths. Return discovered API
routes/specs and token/role context to the parent for API work.

Use available browser/proxy/MCP tools for actual traffic and DOM behavior; shell
HTTP clients or crawlers may complement them when installed. Inspect schemas/help
and capabilities first. Do not claim browser execution from an HTTP-only response.
Select finite parameter/path corpora from observed inputs; compare content,
redirects and timing to a baseline, not merely status codes. Avoid unbounded path
brute force or automated form submission without the assigned operation policy.

Validate relevant XSS/DOM encoding, CSRF/session flows, origin/CORS decisions,
upload/path handling, SSRF, caching and proxy/parser boundaries. Use synthetic
markers and controlled endpoints within scope. Distinguish reflection from execution,
missing headers from exploitability, and DNS/HTTP reachability from data access.
Reproduce candidate/control behavior with exact auth state and browser/server
baseline; report untested client behavior and unavailable collaborator capabilities.

Use security-assessment's mandatory filesystem workspace: `.security/status.json`
queues discovered targets by category and `.security/<category>/<safe-target>/attempt-<n>/`
holds redacted evidence, separately for web/API/APK/AD/infrastructure/cloud. Read its `references/workspace.md` before dispatch/mutation.
Work only on the assigned category/target/attempt; send other-target/category discoveries and phase deltas
to the single coordinator writer. Default to sequential targets across categories; preserve existing
files and resume from recorded evidence. Other tracking needs explicit user direction.
