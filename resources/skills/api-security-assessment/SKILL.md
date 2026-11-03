---
name: api-security-assessment
description: Enumerate API contracts and validate identity, object and business-flow boundaries with paired evidence.
source: Dusk Code
tags: [security, assessment]
---

# API assessment

Use security-assessment for stage gates and evidence. Use OWASP API Security as a
risk taxonomy, not an assertion that a fixed checklist proves security.

Reconcile supplied OpenAPI/GraphQL/gRPC contracts, source handlers, proxy traffic
and web/mobile leads. Record endpoint or operation, method, content type, parameters,
object IDs, auth scheme, roles/tenants, version and provenance. Introspection and
schema retrieval must follow scope; do not assume enabled features or tool support.

Build a small permission matrix with provided principals and synthetic objects:
owner, same-role non-owner, other tenant, lower privilege, anonymous/expired context
where supplied. Test object, function and property authorization across reads,
writes, lists, exports and nested/batch operations. Verify persisted effects and
response filtering; changing an identifier or receiving 200 alone proves no bypass.

Select targeted malformed inputs and finite fuzz corpora from the contract and
observed validation boundaries. Compare valid/invalid controls. Assess token/session
lifecycle, mass assignment, injection, SSRF, pagination/resource limits and replay or
idempotency when relevant. Use small deterministic requests and synthetic transactions;
resource exhaustion or financial/business side effects require corresponding scope.

Use exposed API/proxy tools or installed HTTP clients; verify actual schemas and
flags first. Preserve redacted request/response and state evidence with principal,
object and baseline IDs. Hand off unresolved server prerequisites and client leads;
separate contract discrepancies, confirmed defects and unexecuted hypotheses.

Use security-assessment's mandatory filesystem workspace: `.security/status.json`
queues discovered targets by category and `.security/<category>/<safe-target>/attempt-<n>/`
holds redacted evidence, separately for web/API/APK/AD/infrastructure/cloud. Read its `references/workspace.md` before dispatch/mutation.
Work only on the assigned category/target/attempt; send other-target/category discoveries and phase deltas
to the single coordinator writer. Default to sequential targets across categories; preserve existing
files and resume from recorded evidence. Other tracking needs explicit user direction.
