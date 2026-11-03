---
name: mobile-security-assessment
description: Assess mobile packages, device behavior and backend boundaries with explicit evidence levels.
source: Dusk Code
tags: [security, assessment]
---

# Mobile assessment

Use security-assessment for scope, stage handoffs and evidence. Use OWASP MASTG
and MASVS as assessment references; record applicable platform/build and consulted
reference rather than implying certification or exhaustive coverage.

Inventory platform, package/build/signature, supplied source, device/emulator,
permissions and test identities. Map storage, logs/backups, exported components,
intents/deep links, WebViews/bridges, IPC, native code, transport and backend endpoints.
Package analysis does not establish device or server behavior.

Inspect available device/package/proxy instrumentation and versions before use
(for example adb, platform tooling or instrumentation only if installed and assigned).
Run the assigned app/build on an authorized device when available. Use synthetic
data and paired cases for component exposure, storage access, WebView/origin rules,
session handling and transport behavior. Keep device modifications, rooting,
certificate installation and instrumentation within the supplied method policy.

Pinning bypass, obfuscation absence or extracted client configuration alone does
not establish a server vulnerability. Verify effective access at the relevant
boundary. Send observed backend/API routes with auth context and provenance to the
parent for a separately scoped API assignment. Return package/source, emulator,
physical-device and backend evidence separately with missing runtime coverage,
cleanup and precise reproduction prerequisites.

Use security-assessment's mandatory filesystem workspace: `.security/status.json`
queues discovered targets by category and `.security/<category>/<safe-target>/attempt-<n>/`
holds redacted evidence, separately for web/API/APK/AD/infrastructure/cloud. Read its `references/workspace.md` before dispatch/mutation.
Work only on the assigned category/target/attempt; send other-target/category discoveries and phase deltas
to the single coordinator writer. Default to sequential targets across categories; preserve existing
files and resume from recorded evidence. Other tracking needs explicit user direction.
