# ADR-0003: Default-Deny Tool Approval And Execution Boundary

## Metadata [Required]

- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Date**: 2026-08-12
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`; approval has not occurred
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`; approval has not occurred
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`; approval has not occurred
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Related [Optional]**: [Koduck card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87); `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-2
- **Supersedes [Conditionally Required — this ADR replaces another]**: None
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present
  with complete, verifiable content. Use `None — <reason>` only when the
  template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be
  completed when its stated trigger applies. When the trigger does not apply,
  retain `N/A — <reason>` unless the template explicitly instructs removal or
  retention as inactive future-lifecycle guidance. A missing trigger assessment
  is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance,
  execution, completion, or verification. If retained, it MUST be accurate and
  complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

ADD-0001 CAND-1 is complete and provides the authenticated, provider-neutral
Turn kernel, durable append-before-publish history, and fenced foreground lease
generation required by this task. The kernel intentionally has no tool-call or
MCP execution path. CAND-2 is now the second dependency-ordered development
candidate: it must add tool execution without allowing model output, transport
metadata, a stale Turn owner, or a user-visible approval projection to grant
authority.

This ADR converts that one ADD candidate into one independently reviewable
implementation slice. It owns the detailed contract for C-5 policy, canonical
D-6 Approval Requests, one-attempt D-7 Execution Attempts, the authenticated
approval transport, and adapters that can address Tool or MCP capabilities only
through an isolated executor. The slice establishes the safety boundary but
does not enable a production privileged capability: the initial production
descriptor allowlist is empty, while deterministic fixtures exercise the full
allow, approval, denial, fencing, timeout, cancellation, and output-handling
contract.

## Scope [Required]

In scope:

- Owned action, descriptor, effect, permission-profile, policy-decision,
  Approval Request, Execution Attempt, and audit-event types.
- One C-5 application boundary below every native Tool and MCP invocation,
  integrated with the CAND-1 provider-neutral Turn runner.
- Canonical PostgreSQL D-6/D-7 state, exact-attempt approval binding, immutable
  D-3 projection Items, and current C-6 foreground-lease validation.
- An authenticated REST decision route and durable SSE projections for pending
  approvals and execution terminals; no visual UI is included.
- Consumer-owned Tool/MCP descriptor adapters and one executor client port that
  sends bounded envelopes to an externally isolated Tool service or worker.
- Deterministic production-boundary integration tests using an isolated
  executor harness and a synthetic Tool/MCP inventory.

Out of scope:

- Reusable session-wide or Turn-wide approval grants, approval caching, or
  approval that changes a Permission Profile.
- Enabling any production privileged Tool/MCP descriptor or adding broad host
  filesystem, process, credential, or arbitrary-network access to `koduck-ai`.
- Direct MCP stdio process spawning or direct MCP endpoint access from the AI
  core or presentation adapter.
- Extension discovery, precedence, provenance snapshots, skills, plugins, and
  repository-instruction loading, which remain CAND-4.
- Background Multitask execution, fork/checkpoint semantics, semantic Memory,
  deployment, promotion, and UI design.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Tool usefulness competes with least privilege. | A permissive default lets untrusted model or MCP content create host or external effects. | Unknown descriptors and effects are denied; the production descriptor allowlist starts empty; every enabled descriptor must match a fixed effect and Permission Profile rule. |
| TN-2 | Approval responsiveness competes with durable, multi-instance authority. | Process-local waiters are fast but lose or duplicate authority across crashes and instances. | PostgreSQL D-6/D-7 state is canonical; local notification is only an optimization, and every transition uses a conditional durable write. |
| TN-3 | Cancellation responsiveness competes with truthful effect reporting. | Claiming cancellation after an executor may have started an effect hides partial impact. | The executor reports whether the effect is `not_started`, `started`, or `unknown`; fencing or cancellation after dispatch yields an exact cancelled or failed terminal from that observed state. |
| TN-4 | Retry improves availability but can duplicate privileged effects. | Replaying a started or ambiguous attempt can repeat a non-idempotent action. | At most one automatic retry is allowed only when the executor proves the prior attempt never started an effect; the retry gets a new D-7 identity and fresh policy/approval evaluation. |

### Constraints [Required]

- ADD-0001 remains `Current`, CAND-1 remains `Complete`, and this ADR keeps an
  exact reciprocal link to CAND-2.
- C-5 is the only authority for policy, D-6, and D-7; C-1 transports decisions,
  C-2 orchestrates, C-6 persists, and D-3 Items are non-authoritative views.
- Provider, Tool, MCP, HTTP, PostgreSQL, and executor wire types do not enter
  domain policy types.
- C-2 and C-1 never perform direct host execution or contact an MCP endpoint;
  actual effects cross the consumer-owned executor port into an isolated
  external boundary.
- Signed identity and the `ai.tool.approve` scope come from C-7. Forwarded
  headers or request bodies cannot invent an approver identity or scope.
- A Turn resolves one immutable Permission Profile. Approval authorizes only
  one exact D-7 and never changes or widens that profile.
- Every externally visible approval or execution projection is durably appended
  before publication and references the canonical D-6/D-7 version.
- All maintained Rust changes follow the common and Rust development standards;
  no engineering exception is approved by this ADR.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Which production Tool/MCP capabilities are enabled by this slice? | @linhai | 2026-08-12 | Resolved | None. The initial production descriptor allowlist is empty; synthetic descriptors exist only in tests. Enabling a real privileged capability is outside CAND-2 and requires an Accepted decision for its exact scope. |
| Q-2 | Where do native Tool and MCP effects execute? | @linhai | 2026-08-12 | Resolved | Through one consumer-owned executor client port to an external isolated Tool service or worker. `koduck-ai` has no direct filesystem/process/MCP transport implementation in this slice. |
| Q-3 | What identity can decide a pending approval? | @linhai | 2026-08-12 | Resolved | A C-7-validated principal in the same tenant with `ai.tool.approve`; the canonical request also fixes the owning Thread, Turn, action digest, and D-7 identity. |

## Decision Drivers [Required]

1. **Unbypassable authority**: Every native Tool and MCP path must cross the
   same default-deny policy and executor boundary.
2. **Exact durable authorization**: One approval must authorize exactly one
   immutable attempt and survive multi-instance races without becoming broader.
3. **Fenced ownership**: A stale CAND-1 foreground owner must neither dispatch
   an effect nor commit its result.
4. **Truthful failure semantics**: Timeout, cancellation, executor failure,
   partial/unknown effect state, and untrusted output must remain distinct and
   auditable.
5. **Bounded implementation slice**: The task must extend the existing Rust
   service in one reviewable pull request without enabling production privilege
   or absorbing CAND-3/CAND-4.

## Options Considered [Required]

### Option A: Consumer-owned C-5 state machine with a durable store and isolated executor port

The existing `koduck-ai` crate owns model-neutral policy and lifecycle types,
persists D-6/D-7 through its AI-owned PostgreSQL boundary, and delegates every
effect through one external executor port. Tool and MCP adapters can translate
descriptors and results but cannot dispatch around C-5.

Pros:

- Preserves inward dependencies and gives every execution path one authority.
- Supports exact-attempt approval, lease fencing, and multi-instance races with
  durable conditional transitions.
- Keeps host and MCP execution outside the AI service trust boundary.

Cons:

- Adds durable transitions and cross-boundary latency to every Tool call.
- Requires an executor integration contract even while the production
  capability allowlist remains empty.

### Option B: Put policy and approval in each Tool/MCP adapter

Each native Tool and MCP adapter evaluates its own allowlist, requests approval,
and executes through its preferred transport.

Pros:

- Each adapter can be implemented independently.
- Fewer shared application types are needed initially.

Cons:

- Policy, identity, retry, and audit behavior can drift or be bypassed.
- A new adapter becomes a new security authority and must reproduce fencing.
- It contradicts ADD-0001 C-5 ownership.

### Option C: Treat an approval projection or model confirmation as authority

The Turn history contains a pending approval Item and later content indicating
consent; the runner uses that Item to continue execution.

Pros:

- Reuses the existing history stream with minimal new persistence.

Cons:

- Untrusted content or a stale projection could grant authority.
- It cannot atomically bind identity, exact parameters, scope, and one attempt.
- It violates the D-6 canonical-authority invariant.

## Decision [Required]

**Selected option**: Option A — consumer-owned C-5 state machine with a durable
store and isolated executor port.

**Rationale**: Option A is the only option that centralizes authority without
placing execution power in model, HTTP, Tool, or MCP wire types. Durable
conditional D-6/D-7 transitions align approvals with CAND-1 tenant ownership
and lease generations, while the external executor port makes isolation a real
failure and trust boundary. Starting with an empty production allowlist proves
the mechanism without silently authorizing a new capability.

### Consequences [Required]

Positive:

- Native Tool and MCP requests have one default-deny, exact-attempt policy.
- Approval identity, action scope, lease generation, attempt, and terminal
  evidence are correlated and durable.
- A new adapter cannot grant authority merely by declaring an effect or
  returning instructions in its output.
- CAND-4 can later supply provenance-bearing descriptors without changing C-5.

Negative:

- Tool execution adds PostgreSQL and isolated-executor round trips.
- The first implementation provides no enabled production Tool, so user-visible
  Tool usefulness arrives only through a later accepted capability decision.
- Fencing after an external effect begins may yield a failed attempt whose
  effect is `started` or `unknown`; automated retry is prohibited.

Mitigations:

- Keep D-6/D-7 writes small, indexed, conditional, and covered by PostgreSQL
  integration tests.
- Publish explicit typed denial/failure outcomes rather than hiding the empty
  inventory or executor unavailability.
- Record effect state and require operator/manual reconciliation for ambiguous
  external outcomes; never retry them automatically.

### Detailed Design [Required]

#### Effect And Permission Inventory

| Effect ID | Meaning | Baseline policy |
| --- | --- | --- |
| `read_data` | Read bounded non-secret data from an explicitly configured capability. | Allow without approval only when descriptor ID/version and target are in the effective Permission Profile. |
| `external_write` | Create, update, or delete state outside canonical AI history. | Require exact-attempt approval; no production descriptor enabled by this ADR. |
| `filesystem_write` | Change files through an isolated executor. | Require exact-attempt approval; no production descriptor enabled by this ADR. |
| `process_execute` | Start or signal a process through an isolated executor. | Require exact-attempt approval; no production descriptor enabled by this ADR. |
| `network_egress` | Contact a destination beyond the fixed executor endpoint. | Require exact-attempt approval and a destination-restricted profile; no production descriptor enabled by this ADR. |
| `credential_use` | Use a referenced credential at the executor boundary. | Require exact-attempt approval; secret values never enter D-3/D-6/D-7 or logs; no production descriptor enabled by this ADR. |
| `unknown` | Missing, unsupported, stale, or conflicting effect metadata. | Deny with no executor dispatch and no approval path. |

The production inventory is empty until a later Accepted record identifies an
actual descriptor, target limits, owning service, and verification. Tests use
synthetic descriptors for every policy class. A descriptor is valid only when
its ID is 1–128 ASCII bytes, version is fixed, JSON Schema is valid, declared
effect is one inventory value, and serialized input is at most 65,536 bytes.

#### Canonical Records And State Machines

D-6 contains `approval_id`, tenant/subject/Thread/Turn identity, D-7 attempt ID,
descriptor ID/version, effect, exact action digest, bounded display summary,
Permission Profile ID/version, lease generation, requested/expiry timestamps,
decision identity, status, and monotonically increasing record version. The
action digest covers canonical descriptor ID/version, target, parameters,
effect, profile, Turn, lease generation, and D-7 identity. Any change creates a
new attempt and, when required, a new approval.

D-6 transitions are `requested -> accepted | declined | cancelled | expired`.
Only one conditional transition from `requested` succeeds. Expiry is the
earlier of the Turn deadline and five minutes after request creation. The
authenticated decision route is
`POST /api/v1/ai/approvals/{approval_id}/decisions` with an exact JSON object
containing only `decision: accepted | declined | cancelled`; missing identity
returns `401`, while unknown, cross-tenant, cross-Thread, or unauthorized
approval identity returns an indistinguishable `404` and changes no record.
A duplicate identical decision returns the existing terminal projection; a
conflicting decision returns `409 approval-already-resolved`.

D-7 contains the exact action envelope and digest, effect state, lease
generation, timestamps, status, bounded result metadata, and audit correlation.
Its transitions are `prepared -> running -> succeeded | failed | timed_out |
cancelled`; a declined, cancelled, or expired D-6 transitions its still-prepared
D-7 to `cancelled` with `effect_state=not_started`. One Turn runs at most one
D-7 at a time and creates at most 16 attempts. A Tool action runs for at most 30
seconds, emits at most 1,048,576 serialized output bytes, and has at most one
automatic pre-effect retry. Executor results exceeding the cap are discarded
and recorded as `failed/output_limit_exceeded`.

#### Policy, Fencing, Execution, And Retry

The Turn runner converts a validated model Tool call into an owned Action,
looks up an already-configured descriptor snapshot, and calls C-5. C-5 validates
the descriptor, exact input, immutable profile, tenant, Turn, and current lease
generation before preparing D-7. `unknown` or out-of-profile actions are denied
without D-6 or executor dispatch. An in-profile `read_data` action may dispatch
without D-6. Every privileged effect creates D-6 and waits for its canonical
terminal state.

C-5 validates the same current lease generation immediately before the
conditional `prepared -> running` transition and executor dispatch. Result
commit conditionally validates it again. Fencing before dispatch leaves
`effect_state=not_started` and cancels D-7. Fencing after dispatch commits no
Tool result to the model: an executor-confirmed `not_started` attempt is
`cancelled`; `started` or `unknown` is `failed/owner_fenced_after_dispatch`.

Only an executor response proving `effect_state=not_started` permits one
automatic retry. The retry receives a new D-7 identity, reruns descriptor,
profile, and lease policy, and requires a new D-6 for every approval-required
effect. A `started` or `unknown` attempt is never automatically retried.

An authenticated Turn interruption cancels a requested D-6 and prepared D-7,
or sends one bounded cancel request for a running D-7. Executor acknowledgement
with `not_started` or `started` produces `cancelled` with the reported state. If
the executor does not acknowledge before the 30-second action deadline, D-7 is
`timed_out` with `effect_state=unknown`. The Turn remains subject to CAND-1's
single durable terminal arbitration.

#### Adapter, Projection, And Audit Contracts

Both native Tool and MCP adapters produce the same owned descriptor and Action.
MCP names, schemas, content, errors, and results remain untrusted; an MCP
transport or server declaration cannot alter its configured effect/profile.
`koduck-ai` sends only the bounded owned D-7 envelope to the configured executor
endpoint. It neither spawns an MCP stdio process nor directly contacts an MCP
server in this slice.

D-3 adds append-only `approval_status`, `tool_call`, and `tool_result` payloads.
Each projection carries its canonical D-6/D-7 identity and version and is
published only after durable append. It cannot be read as authorization. Tool
output is an opaque untrusted result supplied to the model only after successful
current-generation commit; it is never parsed as a Permission Profile,
descriptor, instruction source, or approval.

Audit evidence correlates tenant pseudonym, Thread, Turn, descriptor version,
profile version, action digest, lease generation, policy decision, D-6 version,
D-7 transition, executor effect state, timing, byte count, and stable terminal
code. It excludes credentials and minimizes action parameters/result content.

#### Normative Contract Clauses

- **TC-01 — Single authority**: Every native Tool and MCP invocation MUST enter
  C-5; `koduck-ai` MUST contain no direct host-process, filesystem-effect, or
  MCP-server execution path around the executor port.
- **TC-02 — Default deny**: Missing, stale, disabled, incompatible, conflicting,
  or unknown descriptor/effect metadata MUST produce a typed denial, zero D-6,
  and zero executor dispatch.
- **TC-03 — Immutable profile**: One Turn MUST retain one Permission Profile
  ID/version; model, descriptor, Tool/MCP output, and approval MUST NOT widen it.
- **TC-04 — Exact approval**: One accepted D-6 MUST authorize exactly its bound
  tenant, Thread, Turn, lease generation, descriptor version, effect, action
  digest, profile version, and D-7 identity, and no other attempt.
- **TC-05 — Authenticated decision**: Only a C-7-validated same-tenant principal
  with `ai.tool.approve` MAY resolve a requested D-6; invalid ownership or scope
  MUST mutate no state and expose no approval existence.
- **TC-06 — Projection non-authority**: D-3 approval/tool projections MUST be
  append-only views of a canonical D-6/D-7 version and MUST NOT authorize or
  redispatch execution.
- **TC-07 — Lease fencing**: C-5 MUST validate the current foreground lease
  generation at preparation, immediately before dispatch, and result commit;
  a fenced owner MUST commit no Tool result to the model.
- **TC-08 — Retry safety**: Automatic retry MUST occur at most once and only
  after `effect_state=not_started`; it MUST use a new D-7 and fresh policy plus
  a new D-6 when approval is required.
- **TC-09 — Bounded execution**: One Turn MUST have at most one running D-7 and
  16 total attempts; one action MUST stop at 30 seconds and 1,048,576 serialized
  output bytes with an exact terminal status/code.
- **TC-10 — Cancellation truth**: Interruption MUST close a requested approval
  without dispatch or issue one bounded executor cancellation; an unacknowledged
  cancellation at the deadline MUST be `timed_out/effect_state=unknown`.
- **TC-11 — Untrusted result**: Tool/MCP output MUST NOT alter descriptors,
  permission, approval, identity, or execution routing and MUST reach the model
  only after successful current-generation durable result commit.
- **TC-12 — Durable concurrency**: Competing approval decisions, dispatchers,
  terminal results, and reconcilers MUST use conditional canonical transitions;
  exactly one transition wins and no D-7 dispatch occurs twice.
- **TC-13 — Disabled recovery**: The unpromoted dispatcher MUST be disabled by
  removing its runtime enablement; failure MUST leave Tool/MCP unavailable and
  MUST NOT call a predecessor or direct fallback path.
- **TC-14 — Audit minimization**: Every policy/approval/execution terminal MUST
  emit correlated metadata without credential values or unbounded parameters
  and result content.

## Implementation Plan [Required]

**Complete task outcome**: Every native Tool and MCP invocation in the new
Koduck AI Turn lifecycle crosses one default-deny C-5 policy and isolated,
one-attempt D-7 executor boundary; approval-required work uses one canonical
exact-action D-6 and produces bounded, fenced, cancellable, auditable terminal
evidence without enabling a production privileged capability.

**Primary implementation boundary**: C-5 policy, approval, and execution
application boundary in `koduck-ai`; adjacent C-1/C-3/C-6 and runtime changes
are limited to transport, Tool-call input, persistence, and executor wiring
required by that boundary.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Implement owned Tool/MCP action, descriptor, effect, profile, C-5 policy, D-6/D-7 state machines, bounds, retry, cancellation, and lease-fencing behavior. | `koduck-ai` domain/application modules, consumer-owned ports, runner integration, intent-bearing public documentation, focused unit/contract tests. | Not Started | Pending |
| T-2 | Implement authenticated approval and projection transport plus Tool/MCP and isolated-executor adapters with an empty production descriptor allowlist. | REST decision route, SSE/D-3 payloads, provider Tool-call translation, Tool/MCP descriptor adapters, executor client and runtime configuration; no direct host/MCP execution. | Not Started | Pending |
| T-3 | Persist canonical D-6/D-7/audit metadata and prove multi-instance, fencing, production-boundary, and fail-closed behavior. | Idempotent PostgreSQL migration/adapter operations, integration harness, race/fault/limit tests, contract copy and runtime documentation. | Not Started | Pending |

**Affected paths**: `Cargo.lock`; `koduck-ai/Cargo.toml`;
`koduck-ai/migrations/0002_cand_2_policy_execution.sql`;
`koduck-ai/src/domain/**`; `koduck-ai/src/application/**`;
`koduck-ai/src/adapters/http/**`; `koduck-ai/src/adapters/provider/**`;
new `koduck-ai/src/adapters/execution/**` and
`koduck-ai/src/adapters/tool/**`; `koduck-ai/src/adapters/history/**`;
`koduck-ai/src/runtime/**`; `koduck-ai/src/lib.rs`;
`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md`;
`koduck-ai/docs/runtime-configuration.md`; and focused
`koduck-ai/tests/**` fixtures and contract/integration tests. Existing governed
source markers affected by this slice must cite this ADR in addition to any
still-applicable prior ADR.

**Migration and rollback strategy [Conditionally Required — this changes
existing behavior]**: Apply the additive, idempotent D-6/D-7 migration before
runtime enablement. The production descriptor allowlist remains empty, so the
existing CAND-1 tool-free path remains the only executable Turn path until a
later Accepted capability record. Stop on any failed contract, fencing, race,
executor-isolation, or PostgreSQL check. Before promotion, revert or disable the
new dispatcher/configuration and retain the additive tables as unused audit
schema or remove them only through a separately governed compatible migration;
Tool/MCP requests then fail closed as unavailable. There is no predecessor,
direct-execution, or MCP fallback. After a verified artifact is promoted, any
artifact rollback requires an Accepted OCR and may select only a verified new
artifact.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule exception is proposed. Any implementation that
exceeds or waives a rule requires an approval-invalidating ADR update and
reapproval before that source is introduced or retained.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-01 | This ADR — Normative Contract Clauses | All native Tool/MCP invocation paths enter C-5; no direct effect path exists. | AC-1, AC-11 | Dependency/source inspection plus Tool and MCP integration calls prove one executor-port dispatch and zero forbidden APIs. |
| TC-02 | This ADR — Normative Contract Clauses | Unknown or invalid metadata denies with zero approval and dispatch. | AC-2 | A table test supplies every missing/stale/disabled/conflicting case and asserts counters remain zero. |
| TC-03 | This ADR — Normative Contract Clauses | Turn profile is immutable and cannot be widened by untrusted content or approval. | AC-3, AC-10 | Mutation attempts from model, descriptor, decision, and result fixtures leave the fixed profile ID/version and decision unchanged. |
| TC-04 | This ADR — Normative Contract Clauses | Accepted D-6 authorizes exactly one fully matching D-7. | AC-4, AC-5 | Exact match executes once; field-by-field drift cases create no dispatch and cannot reuse the approval. |
| TC-05 | This ADR — Normative Contract Clauses | Only a validated same-tenant scoped approver resolves D-6 without existence leakage. | AC-6 | HTTP contract cases cover missing identity, wrong tenant/Thread, missing scope, valid decision, duplicate, and conflict. |
| TC-06 | This ADR — Normative Contract Clauses | D-3 is durable append-only projection and never authority. | AC-3, AC-7 | Forged/stale projection does not dispatch; real transitions append ordered versions before SSE publication. |
| TC-07 | This ADR — Normative Contract Clauses | Current lease is checked at prepare, dispatch, and commit; fenced results do not reach the model. | AC-5, AC-8 | Fence injection at each boundary asserts exact D-7 terminal/effect state and zero result delivery. |
| TC-08 | This ADR — Normative Contract Clauses | Only one proven-pre-effect retry uses new D-7 and fresh policy/approval. | AC-9 | Retry fixtures compare identities and approval counts for not-started, started, and unknown failures. |
| TC-09 | This ADR — Normative Contract Clauses | Concurrency, attempt, time, and output bounds are exact. | AC-10 | Virtual-time and byte/count boundary tests exercise values at and above each limit. |
| TC-10 | This ADR — Normative Contract Clauses | Cancellation closes pending work or produces truthful bounded executor outcome. | AC-8 | Pending, running-acknowledged, and running-unacknowledged cases assert terminal states and dispatch/result counts. |
| TC-11 | This ADR — Normative Contract Clauses | Tool/MCP output cannot grant authority and is delivered only after fenced durable commit. | AC-3, AC-8 | Malicious output fixtures plus append/fence traces verify immutable authority and append-before-model delivery. |
| TC-12 | This ADR — Normative Contract Clauses | Conditional durable transitions permit one winner and no duplicate dispatch. | AC-4, AC-12 | 32-way PostgreSQL races cover decisions, dispatch claims, and terminal commits. |
| TC-13 | This ADR — Normative Contract Clauses | Disabled recovery exposes unavailability with no legacy or direct fallback. | AC-11 | Runtime/dependency inspection and disabled-executor request assert zero fallback identifiers and dispatches. |
| TC-14 | This ADR — Normative Contract Clauses | Correlated bounded audit excludes credential values and content. | AC-13 | Audit fixture inspection asserts required IDs, hashes, byte counts, terminal code, and absent secret/raw content. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | Two approvers and 32 dispatchers race for one requested D-6/prepared D-7 while a terminal result also races. | C-5 plus C-6 PostgreSQL adapter | AC-12 production PostgreSQL race test. | One D-6 decision and one dispatch claim win; one D-7 terminal is durable; executor dispatch count is exactly 1; every loser returns the existing terminal or typed conflict. | AC-12 | Not Started | Not run — implementation not started |
| timeout and deadline | Approval expires at five minutes or Turn deadline; executor crosses 30 seconds. | C-5 clock/deadline policy and executor adapter | Virtual-clock boundary cases in AC-10. | At the exact deadline pending D-6 is expired with zero dispatch; running D-7 is `timed_out`; no late result reaches the model. | AC-10 | Not Started | Not run — implementation not started |
| cancellation and interruption | Turn is interrupted while approval is pending, while effect is not started, after effect started, and while cancel acknowledgement is lost. | C-2/C-5 cancellation and executor adapter | Fault-driven cases in AC-8. | Pending work dispatches zero times; acknowledged running work is `cancelled` with reported effect state; unacknowledged work is `timed_out/unknown`; one CAND-1 Turn terminal remains durable. | AC-8 | Not Started | Not run — implementation not started |
| resource bounds and backpressure | A Turn requests attempt 17, two concurrent attempts, 65,537-byte input, or 1,048,577-byte output. | C-5 policy and executor envelope/result adapters | Exact boundary table in AC-10. | Each over-limit case is rejected or failed with its stable limit code, zero over-limit bytes reach history/model, and at most one D-7 is running. | AC-10 | Not Started | Not run — implementation not started |
| framework or trust-boundary rejection | Missing/forged identity, cross-tenant decision, malicious MCP descriptor/result, and direct-executor bypass are attempted. | C-1/C-7, C-5, Tool/MCP/executor adapters | Axum HTTP contract, malicious adapter fixtures, and architecture inspection in AC-1/AC-6/AC-11. | Identity cases return exact 401/404 behavior with zero mutation; untrusted content cannot change authority; no forbidden direct execution path exists. | AC-1, AC-6, AC-11 | Not Started | Not run — implementation not started |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Domain/application Tool policy depends on no HTTP, provider-wire, SQLx, Tool/MCP-wire, or executor implementation and every invocation enters C-5. | T-1/T-2 source exists. | Run `cargo test -p koduck-ai --test architecture cand_2_policy_dependencies_are_inward_and_unbypassable -- --exact`. | Exit 0; forbidden-import count is 0; native Tool and MCP entrypoint count equals the count delegating to the C-5 port; direct filesystem/process/MCP execution entrypoint count in C-1/C-2 is 0. | Command output and inspected commit. | Not Started | Pending |
| AC-2 | T-1 | Every missing, stale, disabled, incompatible, conflicting, unknown-effect, or out-of-profile descriptor is denied before approval or execution. | Table fixture contains one case per invalid state. | Run `cargo test -p koduck-ai --test cand_2_policy invalid_descriptors_fail_closed -- --exact`. | Exit 0; each case returns its declared typed denial; D-6 count and executor dispatch count are 0. | Command output and per-case counters. | Not Started | Pending |
| AC-3 | T-1 | Model, descriptor, approval projection, and Tool/MCP result content cannot widen the immutable Turn Permission Profile or authorize execution. | Fixed profile allows synthetic `read_data` only; four malicious fixtures request `process_execute`. | Run `cargo test -p koduck-ai --test cand_2_policy untrusted_content_cannot_grant_authority -- --exact`. | Exit 0; profile ID/version is unchanged; all four privileged requests are denied or require canonical D-6; forged projection causes 0 dispatches. | Command output and decision trace. | Not Started | Pending |
| AC-4 | T-1 | An accepted exact D-6 authorizes one and only one matching D-7. | One approval-required synthetic action with fixed tenant/Thread/Turn/generation/profile/descriptor/target/parameters/attempt. | Run `cargo test -p koduck-ai --test cand_2_approval exact_approval_authorizes_one_attempt -- --exact`. | Exit 0; exact action dispatches once; second use returns `approval-already-consumed`; drift of each bound field dispatches zero times and requires a new policy result/D-6. | Command output, IDs/digests, and dispatch counters. | Not Started | Pending |
| AC-5 | T-1 | A stale owner cannot prepare, dispatch, or commit a Tool result. | Three cases fence the lease immediately before prepare, dispatch, and result commit; post-dispatch fixtures report each effect state. | Run `cargo test -p koduck-ai --test cand_2_fencing stale_owner_never_commits_tool_result -- --exact`. | Exit 0; pre-dispatch cases make 0 executor calls and cancel D-7/not_started; post-dispatch not_started is cancelled, started/unknown is failed `owner_fenced_after_dispatch`; model result count is 0 in every case. | Command output and D-7/history traces. | Not Started | Pending |
| AC-6 | T-2 | The approval decision route enforces exact authenticated ownership, scope, idempotency, and conflict behavior. | Requested D-6 plus missing identity, wrong tenant, wrong Thread, missing `ai.tool.approve`, valid principal, duplicate same decision, and conflicting decision. | Run `cargo test -p koduck-ai --test cand_2_http approval_decision_v1_contract -- --exact`. | Exit 0; missing identity is 401; wrong ownership/scope is indistinguishable 404 with zero mutation; valid and duplicate-identical decisions return the same terminal version; conflicting decision is 409 `approval-already-resolved`. | Command output, normalized response fixtures, and record versions. | Not Started | Pending |
| AC-7 | T-2 | Approval and execution SSE projections are ordered durable views and never independent authority. | One approval-required synthetic Tool call accepted and completed; append/publish observer installed. | Run `cargo test -p koduck-ai --test cand_2_http projections_append_before_publish -- --exact`. | Exit 0; requested, accepted, running, and succeeded projections reference increasing canonical versions; each append precedes publish; deleting/forging/replaying a projection changes no D-6/D-7 and causes zero additional dispatches. | Command output and append/publish trace. | Not Started | Pending |
| AC-8 | T-1 | Interruption and lease fencing produce truthful cancellation outcomes without late result delivery. | Pending approval; prepared attempt; running attempts with acknowledged not_started/started and missing cancellation acknowledgement. | Run `cargo test -p koduck-ai --test cand_2_cancellation`. | Exit 0; pending/prepared cases dispatch 0 and cancel; acknowledged cases are cancelled with exact state; missing acknowledgement reaches `timed_out/unknown` at 30 seconds; late result delivery count is 0; one Turn terminal exists. | Command output, virtual-clock trace, and replay. | Not Started | Pending |
| AC-9 | T-1 | Automatic retry occurs only once after a proven not-started effect and gets fresh authority. | Executor fails before effect, after started effect, and with unknown state; privileged and read-only descriptors. | Run `cargo test -p koduck-ai --test cand_2_retry pre_effect_retry_requires_fresh_attempt_and_policy -- --exact`. | Exit 0; not_started case has exactly two distinct D-7 IDs and, for privileged effect, two distinct accepted D-6 IDs; started/unknown cases have one D-7, no retry, and a typed failed terminal. | Command output and identity/dispatch trace. | Not Started | Pending |
| AC-10 | T-1 | Approval, execution, attempt, input, output, and concurrency limits are exact. | Virtual clock and cases at/beyond five minutes, 30 seconds, attempts 16/17, input 65,536/65,537 bytes, output 1,048,576/1,048,577 bytes, and two simultaneous actions. | Run `cargo test -p koduck-ai --test cand_2_limits exact_policy_and_execution_limits -- --exact`. | Exit 0; at-limit cases follow policy; over-limit cases return exact expiry/timeout/attempt_limit/input_limit/output_limit/concurrent_attempt codes; no over-limit payload reaches model/history and running-attempt count never exceeds 1. | Command output and boundary table. | Not Started | Pending |
| AC-11 | T-2 | Tool and MCP adapters use one isolated executor envelope and the disabled runtime has no direct or predecessor fallback. | Synthetic native Tool and MCP descriptors plus runtime with empty production inventory/executor disabled. | Run `cargo test -p koduck-ai --test cand_2_execution isolated_executor_is_only_effect_path -- --exact` and `cargo test -p koduck-ai --test architecture cand_2_has_no_direct_or_legacy_execution_fallback -- --exact`. | Both exit 0; enabled synthetic calls each yield exactly one identical owned envelope at the harness; disabled runtime returns typed unavailable with 0 dispatches; forbidden direct/legacy identifiers and APIs count is 0. | Command outputs, envelope fixture hash, dependency/config report. | Not Started | Pending |
| AC-12 | T-3 | PostgreSQL permits exactly one decision, dispatch claim, and terminal commit under multi-instance races. | Real PostgreSQL schema; 32 contenders at each transition for one D-6/D-7. | Run the configured PostgreSQL integration test `postgres_cand_2_transitions_are_single_winner`. | Exit 0; each transition has 1 winner and 31 existing-terminal/conflict results; executor dispatch count is 1; replay contains one terminal D-7 projection. | Command output, SQL transition counts, and replay hash. | Not Started | Pending |
| AC-13 | T-3 | Audit metadata is complete, correlated, bounded, and contains no credential or raw unbounded content. | Synthetic credential reference, 65,536-byte parameter boundary, 1,048,576-byte result boundary, and all terminal classes. | Run `cargo test -p koduck-ai --test cand_2_audit audit_is_correlated_and_content_minimized -- --exact`. | Exit 0; every terminal record contains the declared IDs/versions/digest/effect state/timing/byte counts/code; credential value and raw parameter/result substrings occur 0 times; serialized audit record stays below 16,384 bytes. | Command output and redacted audit fixtures. | Not Started | Pending |
| AC-14 | T-3 | The additive migration, runtime contract copy, and all repository Rust checks pass without enabling a production descriptor. | T-1 through T-3 complete; local test database available for integration checks. | Run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; inspect runtime inventory. | All commands exit 0; migration applies twice without error; configured production descriptor count is 0; no disposable build artifact is retained for reuse or promotion. | Command outputs, migration report, inventory inspection, and tested commit. | Not Started | Pending |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Not Started | Pending |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending |
| A-3 | Reciprocal ADD link synchronized | CAND-2 records this exact ADR path, this ADR records the exact ADD path and CAND-2, both references agree, and CAND-2 reaches `Complete` only with this ADR's `Complete` or `Verified` status | ADD candidate row, ADR metadata, and stable revision evidence | Complete | English and Chinese ADD candidate rows record this exact path with `Selected`; ADR metadata and the central index record the exact ADD path and CAND-2. No candidate-completion transition has occurred. |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-12 found every required section present, every conditional trigger assessed, and no unresolved template placeholder. |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | Structured review on 2026-08-12 found 14 checks; each names one T-1/T-2/T-3 subtask, exact inputs, deterministic method, observable result, and evidence. |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception proposed | The subsection records an explicit N/A and requires approval-invalidating reapproval if implementation discovers an exception. |
| A-7 | Contract and baseline risks covered | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Not Started | Pending |

## Supporting Notes [Optional]

- Selection evidence: ADD-0001 is `Current`; CAND-1 is `Complete`; CAND-2 was
  `Ready`; ADR-0001 is `Complete`; ADR-0002 is `Verified`; therefore the
  repository-wide ADR serialization gate permits ADR-0003.
- The Tool-effect inventory classifies effects but enables no production
  descriptor. Test fixtures do not confer runtime authority.
- The repository has configured automatic review evidence on prior pull-request
  revisions. ADR-0003 has no pushed implementation revision yet; exact-revision
  review coverage is therefore pending and remains a later review-ready gate.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

This section is inactive because Decision Status is `Proposed`. When triggered:

- [ ] Move this file to `docs/adr/archive/ADR-0003-default-deny-tool-approval-execution-boundary.md`.
- [ ] Update every governed-source marker and cross-reference to the archived path.
- [ ] If superseded, synchronize reciprocal `Supersedes` / `Superseded By` paths.
- [ ] Retain `Superseded By: None` when no replacement exists.
- [ ] Update this record's single `docs/adr/INDEX.md` row with final status and path.
- [ ] Confirm no live record or code marker still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-12 | Drafted project Full ADR, selected ADD-0001 CAND-2, and defined the default-deny exact-attempt Tool/MCP approval and isolated-execution contract. | @codex |
