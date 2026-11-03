---
name: security-assessment
description: Coordinate scoped security assessments through discovery, enumeration, validation and evidence review.
links: [web-security-assessment, api-security-assessment, mobile-security-assessment, ad-security-assessment, infrastructure-security-assessment]
source: "dusk_code"
license: MIT
tags: [security, assessment]
---

# Assessment stages

Use the parent's established scope, authorization and stricter limits. Resolve only
missing inputs that block an operation: targets/origins/ranges, identities, allowed
methods and effects, exclusions and traffic/time limits. Carry
these forward; discoveries and credentials do not enlarge them.

Before dispatch, read [references/workspace.md](references/workspace.md). Maintain
`.security/status.json` and separate `.security/{web,api,apk,ad,infrastructure,cloud}/<safe-target>/`
folders. Every discovered hostname has its own record/folder in relevant categories;
web and API phases stay independent. Use the queue, single-writer ownership,
atomic updates and resume protocol; default to one active category/target unless
the coordinator sets bounded concurrency.
Dispatch uses the workspace reference's bootstrap-only `ask_agent` followed by a
start grant after actual owner/attempt persistence; children acknowledge ready and
wait, because the initial call starts them before returning their identity.
These are ordinary folders, not Git worktrees. Other tracking requires explicit
user instruction. Never replace existing evidence or put secrets in this workspace.

1. **Scope and capabilities.** Identify baseline/version and environment. Inspect
   exposed tool schemas and installed utilities before choosing them. Prefer an
   available connected capability when suitable; use shell/browser alternatives
   when available. Record missing tools instead of inventing calls or results.
2. **Surface discovery.** Combine supplied inventories, source/configuration,
   manifests/specifications and bounded in-scope observations. Record assets,
   origin/host/service, identity, provenance and coverage. Reconcile discrepancies.
3. **Enumeration and crawl.** Map routes, parameters, objects, service versions and
   trust edges. Bound depth, pages/hosts, requests, concurrency, retries, response
   size and wall time before running. Deduplicate and compare baselines; explicitly
   gate redirects, linked assets, backend hosts and authenticated operations by
   scope. Crawling can trigger state changes: exclude destructive/logout actions
   and honor the assigned operation policy. Fuzz only selected inputs with finite
   corpora and rates, retaining candidate/control pairs. A status anomaly is a lead.
4. **Targeted assessment.** Select hypotheses from observed surface and impact;
   load only the relevant domain skill. Trace source or exercise minimal runtime
   proofs, including a negative/control case. Scanner labels need independent
   validation. Stop when evidence suffices or limits/degradation require a stop.
5. **Verification.** Reproduce findings against the named baseline, check actual
   effects and prerequisites, and retest corrections when assigned. Keep source,
   local fixture, emulator, target runtime and scanner evidence distinct.
6. **Independent review and report.** Return primary evidence to a different
   reviewer through the parent. Report confirmed findings, refuted hypotheses,
   missing coverage, cleanup/residual effects and remediation/retest criteria.
   A clean scoped review is not a claim of complete security.

Stages are an artifact-based workflow, not a runtime phase API. The coordinator
sequences siblings; a domain leaf performs its assigned stages without spawning.
Reuse existing stage evidence when adequate and explain skipped work. Parallelize
only independent surfaces with clear inputs and resource ownership. Send API leads
from web/mobile work to the coordinator with endpoint provenance and auth context;
do not start a blind second scan or silently cross the assigned domain scope.

Read [references/handoff.md](references/handoff.md) when handing off a stage or
reporting findings. Save redacted primary evidence in the assigned category/target/attempt
folder and update the queue through its single coordinator writer. Preserve only
the minimal evidence required by the assignment.
