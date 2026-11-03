---
name: ad-security-assessment
description: Map Active Directory trust and permission paths and validate assigned identity-control hypotheses.
source: Dusk Code
tags: [security, assessment]
---

# Active Directory assessment

Use security-assessment for scope, bounded enumeration and evidence. Treat domain,
forest, tenant, host and identity boundaries as separate scope inputs. Use MITRE
ATT&CK as a technique taxonomy where relevant, not as evidence of compromise.

Start with supplied inventory and authorized directory/DNS observations. Record
domains/trust directions, DCs, LDAP/Kerberos services, groups, delegated rights,
GPOs, certificate services and privileged administration paths relevant to scope.
Prefer read-only queries with provided identities; finite filters/page sizes and
concurrency prevent accidental directory-wide load. Verify tool capability and
collection behavior before invoking directory collectors or graph analysis.

Prioritize effective permissions, delegation/trust misconfiguration, ACL inheritance,
authentication policy and certificate-template leads using actual directory values
and prerequisites. A graph path or tool label is a hypothesis until each necessary
edge, identity context and effective permission is supported. Distinguish readable
metadata from usable credential material and potential privilege from exercised access.

Validate with minimal synthetic or explicitly assigned objects and a denial/control
case. Credential acquisition, password spraying, relay/coercion, ticket abuse or
changes to directory/host state require the corresponding supplied authorization
and limits; do not infer them from an enumeration assignment. Stop at sufficient
proof, record residual effects and return cross-forest/cloud/host leads to the
parent. Redact identity secrets and unrelated directory data from reports.

Use security-assessment's mandatory filesystem workspace: `.security/status.json`
queues discovered targets by category and `.security/<category>/<safe-target>/attempt-<n>/`
holds redacted evidence, separately for web/API/APK/AD/infrastructure/cloud. Read its `references/workspace.md` before dispatch/mutation.
Work only on the assigned category/target/attempt; send other-target/category discoveries and phase deltas
to the single coordinator writer. Default to sequential targets across categories; preserve existing
files and resume from recorded evidence. Other tracking needs explicit user direction.
