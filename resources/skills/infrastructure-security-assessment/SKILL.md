---
name: infrastructure-security-assessment
description: Assess infrastructure, cloud and network exposure with configuration and effective-access evidence.
source: "dusk_code"
license: MIT
tags: [security, assessment]
---

# Infrastructure assessment

Use security-assessment for stages, bounds and evidence. Begin from supplied
ranges/resources and source/IaC/service inventory; reconcile DNS, listeners, routing,
firewall/security-group policy, management endpoints, host/service versions,
containers/workloads and identity relationships. Track cloud accounts/projects,
regions and resource ownership separately; discovered peers are leads for the parent.

Choose exposed provider/network/MCP capabilities or installed protocol clients and
scanners only after capability inspection. Use finite host/port sets, request rates,
timeouts and retries; prefer passive/configuration evidence and read-only account
queries where sufficient. Distinguish blocked, refused, timed-out and reachable
observations. A banner or version match alone does not prove affected configuration
or exploitability; check relevant primary advisories before a vulnerability claim.

Trace effective IAM/service permissions, anonymous/storage access, secret delivery,
network segmentation, workload isolation, supply chain and public management access.
Validate relevant hypotheses with synthetic resources/data and allowed/denied pairs;
separate configured policy from effective runtime behavior. Use actual provider
semantics and record baseline, identity, resource and API/protocol observations.

New roles, credentials, deployments, host changes, disruption and deletion are
separate operations requiring corresponding scope. Return AD or application/API
leads with prerequisites to the parent; do not silently pivot. Report redacted
resource references, evidence level, cleanup, missing access/coverage and correction
or retest criteria, without claiming all infrastructure is secure.

Use security-assessment's mandatory filesystem workspace: `.security/status.json`
queues discovered targets by category and `.security/<category>/<safe-target>/attempt-<n>/`
holds redacted evidence, separately for web/API/APK/AD/infrastructure/cloud. Read its `references/workspace.md` before dispatch/mutation.
Work only on the assigned category/target/attempt; send other-target/category discoveries and phase deltas
to the single coordinator writer. Default to sequential targets across categories; preserve existing
files and resume from recorded evidence. Other tracking needs explicit user direction.
