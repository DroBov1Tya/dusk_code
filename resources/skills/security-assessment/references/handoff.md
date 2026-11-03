# Stage and finding handoffs

Use [workspace.md](workspace.md): `.security/status.json` is the categorized queue,
with evidence under `.security/<category>/<safe-target>/attempt-<n>/`. Web/API/APK/
AD/infrastructure/cloud work has independent target/phase records. Other tracking requires
explicit user instruction. A missing writable workspace blocks dispatch. Use stable
asset/finding IDs and references, not copied raw logs. A path without saved content
is not evidence; never overwrite prior attempts or place secrets in artifacts.

Every stage handoff includes:
- Task ID, category, target ID/canonical hostname, workspace and artifact paths, registry revision,
  phase, actual owner, attempt, baseline/version/time and stage outcome.
- Queue counts and next selected category/target IDs, remaining per-target/host/aggregate budgets,
  checkpoint/cursor, pending coordinator delta and resume/ownership blockers.
- Workers return deltas for their own assignment; the coordinator validates owner/
  attempt and commits the registry atomically before advancing or redispatching.
- Scope and limits used; tool names/versions or exposed capability, method and
  actual request/time counts when available; mark unavailable metrics explicitly.
- Observed assets/entrypoints with provenance, identity/auth context and trust edge.
- Checks performed and evidence references; distinguish observation, hypothesis,
  scanner lead, source-confirmed defect, reproduced defect and verified correction.
- Candidate/control results, negative evidence, errors, untested coverage and reason.
- New leads with exact parent-owned routing, required input and next bounded action.
- Side effects, cleanup and stop conditions reached; remaining scope/limits.

Each finding includes ID, asset/path/endpoint, prerequisites and identity, expected
versus observed behavior, minimal redacted reproduction/source trace, impact,
confidence and evidence level, control/counterevidence, correction and retest.
Use severity appropriate to demonstrated impact; score only with stated assumptions.
For attack chains cite evidence for every required edge, marking unvalidated edges.

Reviewer handoff adds the original request, acceptance criteria, changed artifacts,
interfaces, baseline and implementer/verifier reports. The reviewer inspects primary
products, separates confirmed defects from hypotheses, and records reviewed scope
and unavailable checks. Unresolved blocking findings return to the owner; the parent
accepts or records partial delivery rather than relabeling an unrun check as passed.

For dispatch, the coordinator uses the actual `ask_agent` schema: new work sets
`agent` to the exact reachable specialist name and `task` to the bounded brief;
follow-up sets `thread` to the returned existing worker identity and `agent` to its
matching role. An assignment label inside `task` does not select a specialist.
Check the returned agent/thread identity; omitted `agent` creates `general`.
Handoffs include `assigned_role`, actual returned role/thread, category/target and
attempt; leaves request a named next role from their parent rather than delegating.

Use workspace.md's two-message bootstrap/start-grant protocol: include gate state,
readiness checkpoint, assignment fields and committed grant revision. A launch error
or stale lock alone does not prove absence of work. Reconcile live workers/jobs
and effects before reassignment; fencing accepts current deltas but does not cancel
in-flight activity. Record these behavioral rather than enforced-runtime guarantees.
