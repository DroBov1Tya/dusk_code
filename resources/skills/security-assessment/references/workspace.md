# Filesystem workspace and hostname queue

Use `<assessment-root>/.security/status.json` as the authoritative categorized target queue.
Use `.security/web/<safe-target>/`, `.security/api/<safe-target>/`,
`.security/apk/<safe-target>/`, `.security/ad/<safe-target>/`,
`.security/infrastructure/<safe-target>/` and `.security/cloud/<safe-target>/`.
Each discovered hostname gets a distinct target folder/record in each relevant
category, including pending/out-of-scope discoveries; web and API work for the same
hostname have separate phases and evidence. Unclassified discoveries enter a
coordinator-owned `discovery` category until routed, retaining provenance. These are ordinary folders,
not Git worktrees. Use another tracking method only on explicit user instruction.
An unavailable writable filesystem blocks assessment dispatch; report the blocker.
Use only the fixed category names above plus `discovery`; never derive a category
path from target data. For offline APKs use `apk-<full-lowercase-sha256-of-file>` as
the target ID and `kind=apk`; for a non-host resource use `resource-<sha256-of-exact-
UTF8-resource-ID>` and `kind=resource`, preserving the original ID as redacted data.
Host targets use `kind=hostname` and canonical hostname as ID. Do not invent DNS
names for packages, IP-only targets or cloud IDs. Map related observed hostnames to
separate category/hostname records; related assets do not inherit authorization.

## Names and containment

Accept a hostname, not a URL, path, wildcard or host:port. Classify valid IP
literals as resources rather than DNS hostnames. Parse such observations
separately and preserve their provenance; never concatenate raw input into a path
or shell command. Remove one terminal DNS dot, lowercase ASCII and validate DNS
labels (1–63 ASCII letters/digits/hyphens, no leading/trailing hyphen; total at most
253 characters). Reject empty labels, separators, control characters, percent
escapes, `.`/`..` and other non-host syntax. For Unicode, use an available verified
IDNA conversion with its mode/version recorded, then apply the same validation;
if unavailable or ambiguous, retain a pending normalization observation in the
coordinator's existing discovery artifact without creating a path from it. Validate
existing `xn--` labels with that IDNA policy too. Pin the policy in registry
`normalization` before Unicode/A-label admission; never mix conversion modes on
resume or silently reinterpret an existing canonical binding. Do not equate CNAMEs, shared IPs or parent/subdomains.

Use the canonical hostname as `<safe-target>` when at most 120 characters. Otherwise
use its first 120 ASCII characters plus `--` plus the full lowercase SHA-256 of the
canonical hostname (ASCII bytes). This bounds the component to 186 characters.
Record original and canonical names and the mapping. Before use, verify an exact
binding in `workspace.json`; a path/digest collision is a blocker, never a merge.
Resolve the trusted assessment root once; every created/read artifact must remain
under its `.security` root. Reject symlinks in `.security`, target folders, lock,
category, registry and artifact path components; use verified no-follow/relative file APIs
where available. If tools cannot ensure containment against concurrent replacement,
stop filesystem mutations rather than relying solely on a prior path check.

## Compact registry schema (version 1)

```json
{
  "schema_version": 1,
  "revision": 1,
  "assessment_id": "assessment-001",
  "updated_at": "2026-11-03T12:00:00Z",
  "concurrency": 1,
  "normalization": {"dns": "ascii-lowercase", "idna": null},
  "categories": {
    "web": {
      "targets": {
        "app.example.test": {
          "kind": "hostname",
          "observed_names": ["App.Example.Test."],
          "workspace": "web/app.example.test",
          "related_targets": [],
          "scope": "in",
          "phase": "surface",
          "status": "queued",
          "owner": null,
          "attempt": 0,
          "gate": null,
          "baseline": "scope.md#baseline-1",
          "limits": {"requests": 100, "concurrency": 1, "seconds": 300},
          "artifacts": [],
          "cursor": null,
          "next_action": "enumerate observed routes",
          "blocker": null,
          "updated_at": "2026-11-03T12:00:00Z"
        }
      }
    }
  }
}
```

The example limits are illustrative, not a testing grant or universal defaults.
Set finite task-appropriate limits before dispatch; carry forward stricter supplied
bounds. `concurrency` is the coordinator's explicit active-assignment limit across
all categories; default 1. Per-tool/target/hostname traffic remains separately
bounded and aggregate limits still apply. Categories have their own `targets`
maps using canonical target IDs as keys. `(category, target ID)` is the ownership/
phase key; the same hostname can be at `web/assessment` and `api/surface`.
`related_targets` links existing category/target records, not inferred completion
or shared permission. Enumerate each discovered hostname's category records when
reporting its overall status; complete web work never marks API work complete.
`scope`: `pending|in|out`. `phase`: `scope|surface|enumeration|assessment|verify|review|report`.
`status`: `queued|running|blocked|interrupted|completed|excluded`.
Only `scope=in` can run; newly discovered hosts start `pending/scope/blocked` until
matched to existing authorization or resolved by the parent. `out` becomes excluded.
`owner` is null or an actual worker ID; `attempt` is a monotonic category/target assignment
number. `gate` is null or `waiting_identity|waiting_ready|released`; it is
coordinator-managed protocol state, not a runtime permission control. For existing
version-1 records that predate gates, an absent `gate` reads as null only when the
record is inactive (`queued` with owner null, or `completed`/`excluded`). Do not
rewrite completed history just to add it. A running or other nonterminal owner-bearing record without
a gate is ambiguous and requires reconciliation before further dispatch. Persist
a gate explicitly when reserving any new assignment. `cursor` is a redacted resumable position/reference, never a session token.
`artifacts` contains objects with relative path, phase, attempt, evidence level and
baseline; include only files actually saved. `baseline` references a saved scope/
version record. Null limits/cursors do not imply unlimited work. Host aliases share only a canonical identity, never a mutable phase. Root `revision`
increments on every registry mutation; timestamps use UTC.

Store scope/baseline in `.security/scope.md`. Target `workspace.json` binds
`schema_version`, `assessment_id`, `category`, `kind`, `target_id` and `workspace`.
The binding must match the record exactly. Inside each category/target use
`attempt-<n>/` with `surface.json`, `enumeration.json`, `findings.md` or other named
artifacts actually needed; use exclusive creation and retain prior attempts.
Read existing files first: do not truncate, reset, silently adopt or delete them.
If an existing root/registry belongs to another assessment, has invalid JSON,
unsupported schema, inconsistent binding or unowned existing files, stop and
request reconciliation. Reuse a validated matching assessment and extend its queue.

## Single registry writer and concurrent workers

The coordinator is the **only registry writer**. Workers own distinct category/target/
attempt folders and return structured deltas. Stages for the same category/target serialize; each has at most one owner.
Web/API workers on the same hostname use different folders/records and exchange
referenced leads through the coordinator. Parallel cross-category work requires
explicit coordinator concurrency and nonconflicting external state, shared request
budgets and test identities; serialize if that independence is unproven. A worker
records other-target/category leads in its own artifacts and returns them to the
coordinator; it never claims or modifies another workspace. Bound active assignments
by registry `concurrency`; without an explicit coordinator limit process one target
across all categories at a time. Keep queued
targets visible instead of putting all discovered names in model context.

Use an actual exclusive filesystem lock, such as atomic creation of
`.security/.registry.lock/`, before initializing or mutating shared queue state.
The lock records writer ID, process/session identity, a unique non-secret ownership
token and creation time; create it
exclusively and release only one's own lock in guaranteed cleanup. Contention means
wait boundedly or report blocked. A stale timestamp is insufficient to steal a
lock: establish writer termination, exclude surviving writers/jobs and reconcile
interrupted writes first. If the writer died before its identity record was saved,
unknown ownership is a blocker, not permission to delete the lock. Preserve any
partial temp files for inspection; commit only validated state. Ownership cannot be
proven from a reused PID or elapsed time alone. Missing
reliable exclusive-lock/no-follow capability blocks shared writes.

While locked: read and validate current registry/revision; apply only a delta for
the expected owner, attempt and baseline; reject stale worker results. Write complete
validated JSON to an exclusively created temporary file in `.security`, flush and
sync it, then atomically replace `status.json` on the same filesystem and sync the
directory where supported. Initial creation must not replace an existing registry.
Read back the committed revision and changed category/target entries before acknowledging.
Other readers see the prior or new complete registry. Never let workers race whole
JSON rewrites; an atomic rename alone does not prevent lost updates.

## Bootstrap gate with the actual ask_agent API

`ask_agent` starts a child before returning its identity. There is no reservation
API. Therefore dispatch assessment work in two messages, never as a start-and-work
brief:

1. Under the registry lock, reserve the bounded queue slot, increment attempt,
   create/verify category/target binding and a fresh attempt folder exclusively,
   then persist `status=queued`, `owner=null`, `gate=waiting_identity`, phase,
   baseline and limits. Count pending bootstrap reservations as occupied slots. A zero target-request
   budget forbids target traffic, not authorized local registry/folder bookkeeping
   or bootstrap-only readiness; it never grants assessment actions.
2. Call `ask_agent` with explicit reachable `agent` and a **bootstrap-only** `task`.
   Include category/target, attempt, baseline and scope-reference. Require the child
   to load instructions, acknowledge readiness, then yield without assessment,
   target I/O, artifact writes or further dispatch until a matching start grant.
3. From the tool result obtain the actual agent/thread identity, verify its role,
   and persist that owner with `gate=waiting_ready`, still queued. Await the
   worker's readiness message. If the call errors or identity/readiness is unknown,
   leave a blocked/interrupted reservation and reconcile; do not start replacement
   assessment work merely because no identity was returned.
4. Once ready, atomically persist/read back `status=running`, actual owner,
   `gate=released`, phase, attempt and limits. Send `ask_agent` follow-up using the
   actual `thread`, matching `agent`, and `task` containing a start grant with
   assessment ID, category/target, attempt, owner, phase, baseline and committed
   revision. The child may start when this follow-up arrives; no assumption about
   when the follow-up tool call returns is necessary.
5. Before each bounded work batch and artifact write, the worker reads and matches
   its assignment fields, released gate and scope against the registry. Revision
   may advance for unrelated targets; it must be at least the grant revision with
   unchanged assignment fields. Missing/mismatched state means stop and report;
   never write the registry. Results identify that same assignment and use only
   its unique attempt folder. The coordinator rejects stale deltas and accepts
   completion only with saved evidence for the matching owner/attempt/baseline.

A ready child waits for the explicit follow-up, even if it observes released state.
Do not resend an uncertain start grant automatically: reconcile actual worker/jobs
state first. Persist the phase outcome and next_action before reserving the next
attempt. Advance only with adequate stage artifacts/control checks, recording any
justified skip. `completed` applies to the category/target only after verification
and independent review/report; intermediate phases return queued, gate null.

This gate is a behavioral convention implemented with existing messages and files,
not an enforced execution barrier. Registry checks fence accepted **state deltas**;
they cannot cancel an in-flight network request/process or stop a noncompliant
worker from writing. Never reassign while the previous worker or its jobs may still
act. Obtain a confirmed safe stop using actually available task/job controls, or
keep the target blocked. Keep prior attempt folders distinct; do not accept their
late artifacts as evidence for a newer assignment without explicit reconciliation.

## Resume, evidence and secrets

On resume, validate the registry and folder bindings, retrieve only the selected
category/target's phase artifacts and bounded queue summary. Reconcile `running` owners with
actual task/job state, including pending bootstrap/released gates. A crash after
persisting released but before delivery does not prove no work ran; a delivery
error does not prove the child never received it. Match readiness, start grants,
worker checkpoints and effects before continuation. Unknown owner liveness becomes
`interrupted`, not automatically
redispatched: checkpoint what may have run, review effects and remaining budgets,
and obtain a safe checkpoint before assigning a fresh attempt. Do not silently
replay external mutations. An older worker's late delta cannot advance a new attempt.
Artifacts completed before a crash may be reconciled after inspection; references
to missing files remain unresolved. Retry only the remaining bounded work, preserving
failed attempts and negative evidence. Record queue counts and next selected category/targets
without copying hundreds of target records into each handoff.

Keep only redacted evidence and non-secret state under `.security`; credentials,
private keys, cookies and raw auth headers belong in the approved external secret
store, referenced by opaque IDs. Redact response bodies and URLs before saving.
Use restrictive local permissions where supported; do not claim they were enforced
without a check. Do not commit `.security` or change repository tracking rules
without the relevant assignment. Record cleanup/residual effects and unavailable
atomicity, permission or runtime guarantees. This is an operational tool workflow,
not an implemented scheduler, locking service or runtime API.

At the end of a bounded coordinator turn, return either the actual saved revision,
assignment/readiness evidence and remaining next action, or a precise blocker with
unchanged/pending state. Capability probes alone are not completion. Do not silently
end or infer success from successful reads; if safe mutation cannot be established,
name the missing guarantee and leave the queue visibly unresolved.
