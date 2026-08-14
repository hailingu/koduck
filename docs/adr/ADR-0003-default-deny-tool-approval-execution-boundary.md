# ADR-0003: Default-Deny Tool Approval And Execution Boundary

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-08-12
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-12T17:56:05+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`, not `Rejected`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`, not `Rejected`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`, not `Rejected`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`; the record has not been retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`; the record has not been retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`; the record has not been retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`; the record has not been retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `In Progress`, not `Blocked`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `In Progress`, not `Blocked`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `In Progress`, not `Blocked`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `In Progress`, not `Blocked`
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
and recorded as `failed/output_limit_exceeded`. Every initial execution and
every retry consumes one of the 16 D-7 attempt slots. If allocating a retry
would exceed that Turn budget, no retry record or dispatch is created and the
current action terminates with `failed/attempt_limit`.

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
effect. It counts as a second attempt against the Turn's 16-attempt budget. A
`started` or `unknown` attempt is never automatically retried.

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
code. One serialized audit record MUST be at most 16,384 bytes. It excludes
credential values and raw action parameters or result content; hashes, byte
counts, stable codes, and bounded display summaries provide correlation.

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
  a new D-6 when approval is required, and each retry MUST consume one of the
  Turn's 16 D-7 attempt slots.
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
  exactly one transition wins and no single D-7 attempt dispatches twice.
- **TC-13 — Disabled recovery**: The unpromoted dispatcher MUST be disabled by
  removing its runtime enablement; failure MUST leave Tool/MCP unavailable and
  MUST NOT call a predecessor or direct fallback path.
- **TC-14 — Audit minimization**: Every policy/approval/execution terminal MUST
  emit at most 16,384 serialized bytes of correlated metadata without credential
  values or raw action parameters and result content.

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
| T-1 | Implement owned Tool/MCP action, descriptor, effect, profile, C-5 policy, D-6/D-7 state machines, bounds, retry, cancellation, and lease-fencing behavior. | `koduck-ai` domain/application modules, consumer-owned ports, runner integration, intent-bearing public documentation, focused unit/contract tests. | In Progress | Test-first source now defines owned descriptors/actions/effects/profiles, default-deny decisions, exact target-scoped Permission Profile ID/version binding, and adapter-validated JSON/object-schema inputs with separate 65,536-byte action-input and descriptor-schema limits enforced before parsing and arbitrary-precision decimal text retained. Every untrusted action-envelope field is bounded before hashing or D-7 allocation: descriptor ID and version are capped at 128 bytes and the exact target at 256 bytes with ASCII/control-character rejection, and the Permission Profile allowlist validates each entry against the same action bound so an oversized configured entry cannot widen the envelope. The Permission Profile ID and version are bounded at 128 bytes with ASCII/control-character rejection through one shared validator applied by both the profile constructor and the exact-action binding, so the profile identity is bounded before it is hashed or retained in D-6/D-7 state. Both the JSON adapter and domain schema constructor reject duplicate properties before policy evaluation. Non-authoritative `ToolPolicy` evaluation can no longer seal a binding: the crate-owned sealing service resolves descriptor/profile snapshots through a crate-owned configuration port, rechecks exact profile identity/version, and writes a private approval requirement before D-6/D-7 creation. The approval-requirement setter is single-call-site guarded so no Tool/MCP adapter can self-authorize a privileged binding. Denied bindings allocate neither, and authorized `read_data` dispatches without D-6. Conditional/idempotent approval resolution requires a crate-owned C-7 authorizer and decision service while independently enforcing same-tenant and same-Thread ownership. Source and focused tests define one explicitly injected strong Turn authority root; its constructor is crate-owned and it is not global, so public callers cannot construct a second root to reset duplicate-D-7 rejection, one-running-attempt arbitration, or the 16-slot budget. The root strongly retains process-local state across temporary handle loss; reclamation is intentionally unavailable until T-3 can prove canonical Turn terminal status and prevent budget resurrection. All executor success and failure paths validate the current lease immediately before terminal commitment. Guarded transitions, exact `concurrent_attempt`, earlier-deadline expiry, and a canonical SHA-256 action digest have focused evidence. A C-5 tool-execution driver now orchestrates authorize, prepare, approve, and execute with exactly one proven-pre-effect retry (TC-08): it retries only on a committed executor `Failed{effect_state=NotStarted}` terminal (never on a success or cancellation), allocating a fresh D-7 identity, rerunning descriptor/profile policy, and creating a fresh D-6 for approval-required effects; a declined, cancelled, or expired D-6 (including a decision arriving after the D-6 expiry) cancels the still-prepared D-7 to `cancelled/not_started` without dispatch via a guarded coordinator path; a retry that exhausts the 16-slot budget returns `failed/attempt_limit`; a controlled clock is re-read at each D-6 creation and dispatch (with the approval decision carrying its own decision time) so a delayed approval cannot pin the D-6 window or D-7 start time to the original call time; and an owner fenced during retry preparation returns an error and delivers no stale committed result. Focused internal fixtures cover the retry logic (`cargo test -p koduck-ai --lib cand_2_retry_tests`: not-started success, started/unknown, at-most-once, fresh D-6, budget-exhausted `failed/attempt_limit`, reconciliation, declined/cancelled cancellation, success/cancellation no-retry, and late-decision expiry); these are logic-level coverage only — AC-9's end-to-end retry verification through the public runtime/Runner entry remains a T-2 deliverable, exercised by a black-box `tests/cand_2_retry.rs` once that public boundary exists. Cancellation and timeout logic is now implemented: the executor port carries one bounded cancellation behind an opaque permit that reports the acknowledged effect state or the absence of any acknowledgement at the 30-second action deadline; the coordinator validates the current lease before cancelling a running D-7, commits `cancelled` with the executor-reported state, commits `timed_out/unknown` when the cancellation is unacknowledged, and re-reads the controlled clock after every executor response — including the bounded cancellation response — so a cancellation acknowledgement that arrives after the 30-second action deadline commits `timed_out/unknown` instead of `cancelled`, and an action whose observed completion reaches 30 seconds commits `timed_out` with truthful effect-state evidence instead of a succeeded or failed result; an interruption handle resolves the Turn's cataloged prepared or running D-7 through the shared process authority and closes it via the same guarded conditional-commit path, a fenced owner receives a reconciliation requirement while the running attempt stays cataloged, and a late executor response for a cancelled D-7 is rejected without model delivery. `DisabledExecutor` reports its cancellation boundary as unavailable, so C-5 retains the running D-7 for reconciliation instead of fabricating a timeout before the action deadline; the disabled runtime still exposes no effect path. Focused internal fixtures cover the cancellation and timeout contract (`cargo test -p koduck-ai --lib cand_2_cancellation_tests`: prepared interruption with zero dispatch and zero cancel calls, acknowledged not-started and started terminals, unacknowledged `timed_out/unknown`, fenced-owner reconciliation retaining the running attempt, the exact 30-second deadline boundary on both sides, late-result rejection after cancellation, non-running typed rejection, and the unknown-Turn no-op); like the retry fixtures these are logic-level coverage only — AC-8's end-to-end cancellation verification through the authenticated transport remains a T-2 deliverable. Production root/runner wiring, PostgreSQL authority and safe reclamation, and runner integration remain incomplete. |
| T-2 | Implement authenticated approval and projection transport plus Tool/MCP and isolated-executor adapters with an empty production descriptor allowlist. | REST decision route, SSE/D-3 payloads, provider Tool-call translation, Tool/MCP descriptor adapters, executor client and runtime configuration; no direct host/MCP execution. | In Progress | Added duplicate-aware fail-closed JSON-Schema translation with a pre-deserialization 65,536-byte schema limit, the consumer-owned isolated executor port protected by an opaque coordinator-only dispatch permit, and an incremental response builder that rejects output before buffering beyond 1,048,576 bytes and cannot finish after overflow. The conditional terminal-commit port distinguishes a winning write, an existing canonical terminal, a conflicting terminal, fencing, and unavailability. Existing terminals are reconstructed only through a type that validates nonzero canonical version and the output cap, retain the exact D-7 binding, and are rejected for reconciliation when that binding differs; the coordinator therefore returns only the matching bounded canonical winner and never a losing local output. A rejected canonical dispatch claim returns the non-final `ExecutionPending::DispatchRejected` path while retaining the prepared or existing canonical D-7 state, so it cannot be mistaken for durably committed Tool output, and a sealed Turn's rejected claim carries its own interruption code rather than an approval-mismatch diagnosis, so callers do not take the approval-failure recovery path. Crate-internal lifecycle methods use unique `claim_dispatch`, `mirror_terminal`, and `allocate_attempt` names, and architecture tests scan the complete production source graph to enforce their one coordinator claim, two conditional-commit call sites, and one lease-validating preparer allocation call site, so no D-7 is allocated without the TC-07 current-generation lease check. The C-7 validated-decision setter is likewise single-call-site guarded, so the approval transport cannot apply a caller-supplied decision without the ApprovalDecisionService and ApprovalAuthorizer. `DisabledExecutor` is the sole production `IsolatedExecutor` implementation, but production runtime wiring remains incomplete; architecture tests scan the complete source graph plus crate manifest for direct/legacy execution paths. Approval HTTP/projection/provider Tool-call wiring remains incomplete pending the C-7 signed-scope adapter. |
| T-3 | Persist canonical D-6/D-7/audit metadata and prove multi-instance, fencing, production-boundary, and fail-closed behavior. | Idempotent PostgreSQL migration/adapter operations, integration harness, race/fault/limit tests, contract copy and runtime documentation. | Not Started | Pending |

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/application/tool_execution.rs` | `ToolExecutionDriver::execute` | N/A — the stable method symbol identifies the orchestration boundary. | C-5 authorization, preparation, approval, dispatch, retry, cancellation, and terminal-result orchestration. | Working tree before the implementation commit. |
| `koduck-ai/src/domain/execution.rs` | `ExactActionBinding` | N/A — the stable type symbol identifies the exact-action authority envelope. | Immutable tenant, Thread, Turn, lease, descriptor, effect, profile, digest, and D-7 binding used by policy and approval. | Working tree before the implementation commit. |
| `koduck-ai/src/domain/execution/authority.rs` | `TurnAuthorityCatalog::authority_for` | N/A — the stable method symbol identifies the shared Turn authority root. | Process-local ownership of duplicate-attempt rejection, running-attempt arbitration, and the bounded attempt budget pending T-3 persistence. | Working tree before the implementation commit. |

**Current T-1 evidence supplement:** C-5 interruption now consumes a separately supplied
`AttemptCancellationService`, so authenticated cancellation can reach its
executor-cancellation client while a dispatch coordinator is blocked. Before
any durable D-7 terminal write, the shared authority atomically reserves the
cataloged `prepared` or `running` state; a dispatch or second cancellation
cannot use a stale mirrored attempt while that transition is in flight. A
running cancellation claims that reservation before it sends its external
cancel request, so a concurrent interrupter cannot send a second cancel. The
interruption view is one authority-lock snapshot: if any live D-7 has a terminal
commit in flight, the entire Turn returns typed reconciliation before it partly
closes another visible attempt. The same catalog lock records an interruption
tombstone before releasing that snapshot, so neither a known nor a previously
unknown interrupted Turn can race a later D-7 allocation. A cancellation whose external request has been
sent retains its reservation through fencing or durable-write failure until
reconciliation, rather than reopening a second cancellation. A
requested D-6 cancellation reports either its own `requested -> cancelled`
winner or an already-resolved canonical D-6; both cases still conditionally
close the associated prepared D-7. Focused tests prove blocked-dispatch
reachability, dispatch-versus-stale-prepared cancellation, and
accepted-D-6-before-dispatch behavior. Runtime assembly of the independent
service remains T-2 work; no AC-8 status is promoted. A conditional-commit
`Conflict` is also treated as evidence that a canonical D-7 terminal has
already won, so its reservation remains held until reconciliation instead of
reopening dispatch or cancellation. C-5 timestamps are reduced to a relative
remaining action budget before crossing either executor boundary; the executor
therefore enforces that budget using its own monotonic clock rather than a
persisted or controlled absolute timestamp. If prepared cancellation loses to
a concurrent `prepared -> running` transition, C-5 refreshes the same cataloged
D-7 and sends its one bounded running cancellation rather than leaving the
running effect unaddressed. If no action budget remains before dispatch, C-5
commits `timed_out/not_started` with zero executor calls. If that pre-dispatch
terminal commit is unavailable after the D-7 has been claimed as running, C-5
retains the reservation for reconciliation so a later interruption cannot send
a cancellation to an executor that was never dispatched.
`DisabledExecutor` now reports its cancellation boundary as unavailable, so
C-5 retains the running D-7 reservation for reconciliation instead of
fabricating `timed_out/unknown` before the action deadline.

**Affected paths**: `Cargo.lock`; `koduck-ai/Cargo.toml`;
`koduck-ai/migrations/0002_cand_2_policy_execution.sql`;
`koduck-ai/src/domain/**`; `koduck-ai/src/application/**`;
`koduck-ai/src/adapters/http/**`; `koduck-ai/src/adapters/provider/**`;
new `koduck-ai/src/adapters/execution/**` and
`koduck-ai/src/adapters/tool/**`; `koduck-ai/src/adapters/history/**`;
`koduck-ai/src/runtime/**`; `koduck-ai/src/lib.rs`;
`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md`;
`koduck-ai/docs/runtime-configuration.md`; focused crate-owned
`koduck-ai/tests/internal/cand_2_*.rs` authority fixtures; and
`koduck-ai/tests/**` contract/integration tests. Existing governed
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
| TC-08 | This ADR — Normative Contract Clauses | Only one proven-pre-effect retry uses new D-7, fresh policy/approval, and one additional Turn attempt slot. | AC-9, AC-10 | Retry fixtures compare identities, approval counts, and attempt-budget consumption for not-started, started, unknown, and exhausted-budget cases. |
| TC-09 | This ADR — Normative Contract Clauses | Concurrency, attempt, time, and output bounds are exact. | AC-10 | Virtual-time and byte/count boundary tests exercise values at and above each limit. |
| TC-10 | This ADR — Normative Contract Clauses | Cancellation closes pending work or produces truthful bounded executor outcome. | AC-8 | Pending, running-acknowledged, and running-unacknowledged cases assert terminal states and dispatch/result counts. |
| TC-11 | This ADR — Normative Contract Clauses | Tool/MCP output cannot grant authority and is delivered only after fenced durable commit. | AC-3, AC-8 | Malicious output fixtures plus append/fence traces verify immutable authority and append-before-model delivery. |
| TC-12 | This ADR — Normative Contract Clauses | Conditional durable transitions permit one winner and no duplicate dispatch of the same D-7. | AC-4, AC-12 | Exact approval reuse and 32-way PostgreSQL races cover decisions, per-attempt dispatch claims, and terminal commits while allowing only a new D-7 identity for the TC-08 retry. |
| TC-13 | This ADR — Normative Contract Clauses | Disabled recovery exposes unavailability with no legacy or direct fallback. | AC-11 | Runtime/dependency inspection and disabled-executor request assert zero fallback identifiers and dispatches. |
| TC-14 | This ADR — Normative Contract Clauses | Each correlated audit record is at most 16,384 bytes and excludes credential values and raw action parameters/result content. | AC-13 | Audit fixture inspection asserts required IDs, hashes, byte counts, terminal code, size at/below 16,384 bytes, and absent secret/raw content. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | Two approvers and 32 dispatchers race for one requested D-6/prepared D-7 while a terminal result also races. | C-5 plus C-6 PostgreSQL adapter | AC-12 production PostgreSQL race test. | One D-6 decision and one dispatch claim win; one D-7 terminal is durable; executor dispatch count is exactly 1; every loser returns the existing terminal or typed conflict. | AC-12 | Not Started | Not run — the T-3 PostgreSQL transition adapter and 32-dispatcher production race harness are not implemented |
| timeout and deadline | Approval crosses either the five-minute age limit or an earlier Turn deadline; executor crosses 30 seconds. | C-5 clock/deadline policy and executor adapter | Virtual-clock boundary cases for both approval-expiry legs and execution timeout in AC-10. | At the earlier applicable deadline pending D-6 is expired with zero dispatch; running D-7 is `timed_out`; no late approval or result reaches the model. | AC-10 | Not Started | Not run — focused approval-expiry checks exist, but the complete virtual-clock approval and 30-second executor-deadline table is not implemented |
| cancellation and interruption | Turn is interrupted while approval is pending, while effect is not started, after effect started, and while cancel acknowledgement is lost. | C-2/C-5 cancellation and executor adapter | Fault-driven cases in AC-8. | Pending work dispatches zero times; acknowledged running work is `cancelled` with reported effect state; unacknowledged work is `timed_out/unknown`; one CAND-1 Turn terminal remains durable. | AC-8 | Not Started | Not run at the AC-8 end-to-end boundary — the authenticated cancellation transport is a T-2 deliverable. The internal cancellation and timeout contract is implemented and covered by focused logic-level fixtures (`cargo test -p koduck-ai --lib cand_2_cancellation_tests`: prepared interruption with zero dispatch and zero cancel calls, acknowledged not-started/started terminals, unacknowledged `timed_out/unknown`, the exact 30-second deadline boundary on both sides, late-result rejection after cancellation, and fenced-owner reconciliation); these do not promote AC-8 because they do not exercise the authenticated transport. |
| resource bounds and backpressure | A Turn requests attempt 17, two concurrent attempts, 65,537-byte input, or 1,048,577-byte output. | C-5 policy and executor envelope/result adapters | Exact boundary table in AC-10. | Each over-limit case is rejected or failed with its stable limit code, zero over-limit bytes reach history/model, and at most one D-7 is running. | AC-10 | Not Started | Not run — focused attempt, input, output, and concurrency regressions exist, but the complete AC-10 boundary table and history/model delivery assertions are not implemented |
| framework or trust-boundary rejection | Missing/forged identity, cross-tenant decision, malicious MCP descriptor/result, and direct-executor bypass are attempted. | C-1/C-7, C-5, Tool/MCP/executor adapters | Axum HTTP contract, malicious adapter fixtures, and architecture inspection in AC-1/AC-6/AC-11. | Identity cases return exact 401/404 behavior with zero mutation; untrusted content cannot change authority; no forbidden direct execution path exists. | AC-1, AC-6, AC-11 | Not Started | Not run — domain, adapter, and no-bypass checks exist, but the authenticated HTTP contract and complete malicious descriptor/result fixtures are not implemented |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Domain/application Tool policy depends on no HTTP, provider-wire, SQLx, Tool/MCP-wire, or executor implementation and every invocation enters C-5. | T-1/T-2 source exists. | Run `cargo test -p koduck-ai --test architecture cand_2_policy_dependencies_are_inward_and_unbypassable -- --exact`. | Exit 0; forbidden-import count is 0; native Tool and MCP entrypoint count equals the count delegating to the C-5 port; direct filesystem/process/MCP execution entrypoint count in C-1/C-2 is 0. | Command output and inspected commit. | Not Started | Pending |
| AC-2 | T-1 | Every missing, stale, disabled, incompatible, conflicting, unknown-effect, or out-of-profile descriptor is denied before approval or execution. | Table fixture contains one case per invalid state. | Run `cargo test -p koduck-ai --test cand_2_policy invalid_descriptors_fail_closed -- --exact`. | Exit 0; each case returns its declared typed denial; D-6 count and executor dispatch count are 0. | Command output and per-case counters. | Not Started | Pending |
| AC-3 | T-1 | Model, descriptor, approval projection, and Tool/MCP result content cannot widen the immutable Turn Permission Profile or authorize execution. | Fixed profile allows synthetic `read_data` only; four malicious fixtures request `process_execute`. | Run `cargo test -p koduck-ai --test cand_2_policy untrusted_content_cannot_grant_authority -- --exact`. | Exit 0; profile ID/version is unchanged; all four privileged requests are denied or require canonical D-6; forged projection causes 0 dispatches. | Command output and decision trace. | Not Started | Pending |
| AC-4 | T-2 | An accepted exact D-6 authorizes one and only one matching D-7 through the executor adapter. | One approval-required synthetic action with fixed tenant/Thread/Turn/generation/profile/descriptor/target/parameters/attempt and the isolated-executor harness. | Run `cargo test -p koduck-ai --lib cand_2_approval_tests::exact_approval_authorizes_one_attempt -- --exact`. | Exit 0; exact action dispatches once; second use returns `approval-already-consumed`; drift of each bound field dispatches zero times and requires a new policy result/D-6. | Command output, IDs/digests, and dispatch counters. | Not Started | Pending |
| AC-5 | T-2 | A stale owner cannot prepare, dispatch, or commit a Tool result through the executor adapter. | Isolated-executor harness; three cases fence the lease immediately before prepare, dispatch, and result commit; post-dispatch fixtures report each effect state. | Run `cargo test -p koduck-ai --test cand_2_fencing stale_owner_never_commits_tool_result -- --exact`. | Exit 0; pre-dispatch cases make 0 executor calls and cancel D-7/not_started; post-dispatch not_started is cancelled, started/unknown is failed `owner_fenced_after_dispatch`; model result count is 0 in every case. | Command output and D-7/history traces. | Not Started | Pending |
| AC-6 | T-2 | The approval decision route enforces exact authenticated ownership, scope, idempotency, and conflict behavior. | Requested D-6 plus missing identity, wrong tenant, wrong Thread, missing `ai.tool.approve`, valid principal, duplicate same decision, and conflicting decision. | Run `cargo test -p koduck-ai --test cand_2_http approval_decision_v1_contract -- --exact`. | Exit 0; missing identity is 401; wrong ownership/scope is indistinguishable 404 with zero mutation; valid and duplicate-identical decisions return the same terminal version; conflicting decision is 409 `approval-already-resolved`. | Command output, normalized response fixtures, and record versions. | Not Started | Pending |
| AC-7 | T-2 | Approval and execution SSE projections are ordered durable views and never independent authority. | One approval-required synthetic Tool call accepted and completed; append/publish observer installed. | Run `cargo test -p koduck-ai --test cand_2_http projections_append_before_publish -- --exact`. | Exit 0; requested, accepted, running, and succeeded projections reference increasing canonical versions; each append precedes publish; deleting/forging/replaying a projection changes no D-6/D-7 and causes zero additional dispatches. | Command output and append/publish trace. | Not Started | Pending |
| AC-8 | T-2 | Authenticated interruption and lease fencing produce truthful executor-cancellation outcomes without late result delivery. | Approval transport and isolated-executor harness; pending approval; prepared attempt; running attempts with acknowledged not_started/started and missing cancellation acknowledgement. | Run `cargo test -p koduck-ai --test cand_2_cancellation`. | Exit 0; pending/prepared cases dispatch 0 and cancel; acknowledged cases are cancelled with exact state; missing acknowledgement reaches `timed_out/unknown` at 30 seconds; late result delivery count is 0; one Turn terminal exists. | Command output, virtual-clock trace, and replay. | Not Started | Pending |
| AC-9 | T-2 | Automatic retry occurs only once after a proven not-started executor effect, gets fresh authority, and consumes another attempt slot. | Executor fails before effect, after started effect, and with unknown state; privileged and read-only descriptors; one case begins with 15 prior attempts so the initial action consumes slot 16. | Run `cargo test -p koduck-ai --test cand_2_retry pre_effect_retry_requires_fresh_attempt_and_policy -- --exact`. | Exit 0; with budget available, not_started has exactly two distinct D-7 IDs, consumes two slots, and privileged effect has two distinct accepted D-6 IDs; started/unknown has one D-7 and no retry; after initial slot 16, retry allocation/dispatch count is 0 and the action is `failed/attempt_limit`. | Command output and identity/approval/attempt/dispatch trace. | Not Started | Pending |
| AC-10 | T-1 | Approval, execution, attempt, input, output, and concurrency limits are exact. | Virtual clock; approval cases at 4:59.999/5:00.000 with a later Turn deadline and at 1:59.999/2:00.000 with a two-minute Turn deadline; execution at/beyond 30 seconds; attempts 16/17; input 65,536/65,537 bytes; output 1,048,576/1,048,577 bytes; and two simultaneous actions. | Run `cargo test -p koduck-ai --test cand_2_limits exact_policy_and_execution_limits -- --exact`. | Exit 0; an approval expires at exactly five minutes when its Turn deadline is later and at exactly two minutes when that Turn deadline is earlier, with zero dispatch in both expired cases; all other at-limit cases follow policy; over-limit cases return exact timeout/attempt_limit/input_limit/output_limit/concurrent_attempt codes; no over-limit payload reaches model/history and running-attempt count never exceeds 1. | Command output and boundary table covering both approval-expiry legs. | Not Started | Pending |
| AC-11 | T-2 | Tool and MCP adapters use one isolated executor envelope and the disabled runtime has no direct or predecessor fallback. | Synthetic native Tool and MCP descriptors plus runtime with empty production inventory/executor disabled. | Run `cargo test -p koduck-ai --test cand_2_execution isolated_executor_is_only_effect_path -- --exact` and `cargo test -p koduck-ai --test architecture cand_2_has_no_direct_or_legacy_execution_fallback -- --exact`. | Both exit 0; enabled synthetic calls each yield exactly one identical owned envelope at the harness; disabled runtime returns typed unavailable with 0 dispatches; forbidden direct/legacy identifiers and APIs count is 0. | Command outputs, envelope fixture hash, dependency/config report. | Not Started | Pending |
| AC-12 | T-3 | PostgreSQL permits exactly one decision, dispatch claim, and terminal commit under multi-instance races. | Fresh PostgreSQL database with `KODUCK_AI_TEST_DATABASE_URL` set; migration not previously applied; 32 contenders at each transition for one D-6/D-7. | Run `cargo test -p koduck-ai --test postgres_cand_2 postgres_cand_2_transitions_are_single_winner -- --exact`. | Exit 0; migration succeeds; each transition has 1 winner and 31 existing-terminal/conflict results; executor dispatch count for that D-7 is 1; replay contains one terminal D-7 projection. | Command output, SQL transition counts, and replay hash. | Not Started | Pending |
| AC-13 | T-3 | Audit metadata is complete, correlated, bounded, and contains no credential or raw unbounded content. | Synthetic credential reference, 65,536-byte parameter boundary, 1,048,576-byte result boundary, and all terminal classes. | Run `cargo test -p koduck-ai --test cand_2_audit audit_is_correlated_and_content_minimized -- --exact`. | Exit 0; every terminal record contains the declared IDs/versions/digest/effect state/timing/byte counts/code; credential value and raw parameter/result substrings occur 0 times; serialized audit record is at most 16,384 bytes. | Command output and redacted audit fixtures. | Not Started | Pending |
| AC-14 | T-3 | The additive migration, runtime contract copy, and all repository Rust checks pass without enabling a production descriptor. | T-1 through T-3 complete; local test database available for integration checks. | Run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; inspect runtime inventory. | All commands exit 0; migration applies twice without error; configured production descriptor count is 0; no disposable build artifact is retained for reuse or promotion. | Command outputs, migration report, inventory inspection, and tested commit. | Not Started | Pending |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | `@linhai` self-declared in the active ADR-0003 approval context and then supplied exact `Approve`; metadata records `2026-08-12T17:56:05+08:00`. No Approval Context Revision is recorded because the approved uncommitted revision has no immutable commit yet. |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending |
| A-3 | Reciprocal ADD link synchronized | CAND-2 records this exact ADR path, this ADR records the exact ADD path and CAND-2, both references agree, and CAND-2 reaches `Complete` only with this ADR's `Complete` or `Verified` status | ADD candidate row, ADR metadata, and stable revision evidence | Complete | English and Chinese ADD candidate rows record this exact path with `Selected`; ADR metadata and the central index record the exact ADD path and CAND-2. No candidate-completion transition has occurred. |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-12 found every required section present, every conditional trigger assessed, and no unresolved template placeholder. |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | Structured review on 2026-08-12 found 14 checks; each names one T-1/T-2/T-3 subtask, exact inputs, deterministic method, observable result, and evidence. |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception proposed | The subsection records an explicit N/A and requires approval-invalidating reapproval if implementation discovers an exception. |
| A-7 | Contract and baseline risks covered | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Not Started | Pre-approval review confirmed all TC-01 through TC-14 clauses map to AC-1 through AC-14 and all five baseline risk rows are structurally complete. Runtime `Pass` evidence remains required before review-ready or completion, so this checklist item is not yet complete. |

## Supporting Notes [Optional]

- Selection evidence: ADD-0001 is `Current`; CAND-1 is `Complete`; CAND-2 was
  `Ready`; ADR-0001 is `Complete`; ADR-0002 is `Verified`; therefore the
  repository-wide ADR serialization gate permits ADR-0003.
- The Tool-effect inventory classifies effects but enables no production
  descriptor. Test fixtures do not confer runtime authority.
- Stable implementation touchpoints for the current T-1/T-2 source revision:

  | Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
  | --- | --- | --- | --- | --- |
  | `koduck-ai/src/domain/execution.rs` | `TurnExecutionAuthority::claim_dispatch` | N/A — the stable method identifies the guarded transition | Sole lease- and approval-validating D-7 dispatch claim | Current uncommitted task revision; replace with the implementation commit before completion |
  | `koduck-ai/src/domain/execution/authority.rs` | `TurnExecutionAuthority::interruption_snapshot`, `TurnExecutionAuthority::reserve_terminal`, `TurnExecutionAuthority::mirror_terminal` | N/A — stable methods identify the authority boundary | Atomic interruption view and terminal reservation/reconciliation ownership | Current uncommitted task revision; replace with the implementation commit before completion |
  | `koduck-ai/src/application/execution.rs` | `ExecutionCoordinator::execute` | `let deadline = ActionDeadline::from_started_at(started_at_millis, now());` | Converts the C-5 timestamp into a relative executor budget before dispatch | Current uncommitted task revision; replace with the implementation commit before completion |
  | `koduck-ai/src/application/cancellation.rs` | `ExecutionInterrupter::interrupt`, `ExecutionCoordinator::cancel_running_attempt` | N/A — stable methods identify the ordered cancellation boundary | Prevents partial interruption and reserves a running terminal before the external cancel request | Current uncommitted task revision; replace with the implementation commit before completion |
  | `koduck-ai/src/application/terminal.rs` | `ExecutionCoordinator::commit_reserved_terminal` | `let canonical_terminal_known = matches!(error, AttemptCommitError::Conflict);` | Retains authority when a canonical terminal already won and requires reconciliation | Current uncommitted task revision; replace with the implementation commit before completion |
  | `koduck-ai/src/application/tool_execution.rs` | `ToolExecutionDriver::execute` | N/A — the stable method identifies the complete retry sequence | Owns authorize, prepare, approve-or-cancel, dispatch, and the single allowed pre-effect retry | Current uncommitted task revision; replace with the implementation commit before completion |

- Point-in-time decomposition review of the current task revision; these
  measurements are review evidence, not equality assertions that the ADR or
  source must preserve after later edits:
  `koduck-ai/src/domain/execution.rs` is 764 physical lines and
  `koduck-ai/src/application/execution.rs` is 763 physical lines. Both exceed
  the 400-line review threshold and remain below the 800-line exception limit.
  The domain file retains one exact-attempt authority aggregate: shared binding
  identity, D-6 authorization, and D-7 single-dispatch transition that must
  change together to preserve TC-04/TC-08/TC-12.
  `koduck-ai/src/domain/execution/authority.rs` is 232 physical lines and owns multi-Turn lookup,
  strong shared attempt-budget retention, and the cataloged live-D-7
  reconstruction consumed by the C-5 interruption boundary; a reconstructed
  handle grants no lifecycle authority because every guarded transition still
  verifies cataloged membership. Reclamation remains deferred
  until T-3 can bind it to canonical terminal persistence and prevent a
  reclaimed Turn from being recreated with a fresh budget. The
  application file retains the single lease/preparation/dispatch/conditional-
  commit boundary required to make TC-07 unbypassable; splitting its ports from
  the coordinator now would require reviewers to cross modules for one failure
  path without creating an independent lifecycle. The bounded cancellation
  lives in the sibling `koduck-ai/src/application/cancellation.rs` module;
  `koduck-ai/src/application/cancellation.rs` is 450 physical lines, above the
  production-file review threshold and below the exception limit. It retains
  the single interruption/cancellation boundary whose authority snapshot,
  terminal reservation, executor acknowledgement, deadline, and reconciliation
  rules must remain ordered; splitting those phases would create a pass-through
  module without an independent lifecycle. It reserves a running D-7 before its
  external cancel request; the shared `koduck-ai/src/application/terminal.rs` is 113 physical lines and below every
  file review threshold. This module owns the matching
  conditional commit for both dispatch and cancellation, so no second terminal
  path exists. `ExecutionInterrupter::interrupt` is 87 physical lines,
  including its intent and error documentation, above the method review
  threshold and below the exception limit. It keeps one authority-lock snapshot
  and the ordered all-live-D-7 interruption result together; extracting a
  phase would obscure the no-partial-close reconciliation boundary without an
  independent lifecycle. `ExecutionCoordinator::cancel_running_attempt` is 116
  physical lines, including its intent and error documentation, above the
  method review threshold and below the exception limit. It retains the lease
  checks, pre-side-effect terminal reservation, bounded executor cancellation,
  and truthful terminal selection in one ordered boundary.
  `ExecutionCoordinator::commit_reserved_terminal` is 86 physical lines,
  including its intent and error documentation, above the method review
  threshold and below the exception limit. It retains the single conditional
  commit result mapping and reservation-release decision shared by dispatch and
  cancellation; splitting it would create divergent terminal reconciliation
  rules. Cyclomatic complexity is `N/A — no configured complexity tool`; the
  required substitute reviews measured spans of 87, 116, and 86 physical lines
  with maximum executable nesting depths of five, two, and three respectively.
  The interruption method exceeds the nesting review threshold but remains
  below the exception limit; the fifth level is the prepared-attempt race
  recovery branch inside the one ordered iteration, and extracting it would
  separate the stale-prepared-to-running refresh from the atomic snapshot it
  protects. Reassess both modules and these methods
  during T-3 when durable record mapping and executor transport provide real
  extraction boundaries. `koduck-ai/src/domain/tool.rs` is 658 physical lines, above the
  production-file review threshold and below the exception limit. It remains
  one owned Tool-value aggregate whose JSON values and schemas, descriptors,
  actions, and Permission Profiles share validation and policy invariants;
  splitting them now would move those invariants across modules without an
  independent lifecycle. `ExecutionCoordinator::execute` is 115 physical lines,
  including its intent and error documentation, above the method review
  threshold and below the exception limit. It retains one ordered lease-check,
  dispatch, effect-observation, deadline, and conditional-commit sequence;
  extracting a phase would obscure the TC-07 ordering without creating an
  independent owner.
  Cyclomatic complexity is `N/A — no configured complexity tool`; the required
  substitute review measured the 115-line span and maximum executable nesting
  depth of two, below the nesting threshold.
- `ToolExecutionDriver::execute` is 102 physical lines, including its intent and
  error documentation, above the method review threshold and below the exception
  limit. It retains one authorize, prepare, approve-or-cancel, dispatch, and
  conditional-retry sequence whose ordering encodes TC-08 (retry only on a
  committed `Failed{NotStarted}`) and TC-07 (a fenced retry delivers no result);
  extracting a phase would split the retry-decision state across owners without
  an independent lifecycle. Cyclomatic complexity is `N/A — no configured
  complexity tool`; the substitute review measured the 102-line span and maximum
  executable nesting depth of three (loop, match, arm), below the nesting
  threshold.
  `koduck-ai/tests/internal/cand_2_execution.rs` is 1,031 physical lines,
  above the 600-line test review threshold and below the 1,200-line exception
  limit; it remains one cohesive isolated-execution contract harness whose
  executor, lease, committer, policy, and authority fixtures are shared by the
  fencing, bounds, competition, and terminal-result cases. Splitting those
  cases now would duplicate the security-sensitive fixture rather than create
  an independent test boundary. Reassess it with the T-2 transport harness; no
  engineering exception is required at the measured sizes.
- `koduck-ai/tests/internal/cand_2_retry.rs` is 757 physical lines, above the
  600-line test review threshold and below the 1,200-line exception limit. It is
  one cohesive retry-contract harness whose scripted executor, lease, committer,
  policy, and approval fixtures are shared by the not-started, started/unknown,
  at-most-once, fresh-D-6, budget-exhausted, reconciliation, declined/cancelled
  cancellation, success/cancellation no-retry, fence-during-retry, and
  clock-ordering cases. Splitting them now would duplicate the fixture rather
  than create an independent test boundary; reassess it when the T-2 black-box
  `tests/cand_2_retry.rs` lands. No engineering exception is required at the
  measured size.
- `koduck-ai/tests/internal/cand_2_cancellation.rs` is 1,169 physical lines,
  above the 600-line test review threshold and below the 1,200-line exception
  limit. Its 170-line sibling `koduck-ai/tests/internal/cand_2_cancellation_blocking_dispatch.rs`
  isolates the blocking-dispatch concurrency case and the sealed-dispatch-claim
  interruption-code diagnosis while reusing the parent
  module's fixtures; the 46-line
  `koduck-ai/tests/internal/cand_2_cancellation_disabled_executor.rs` sibling
  isolates the production disabled-adapter regression without pushing the
  shared harness past the 1,200-line exception limit. Together they are the logic-level cancellation
  and timeout harness for TC-09/TC-10: its scripted executor, lease, committer,
  and shared-runtime fixtures cover prepared interruption, acknowledged
  not-started/started cancellation, unacknowledged `timed_out/unknown`,
  fenced-owner reconciliation, the exact 30-second deadline crossing,
  late-result rejection, non-running typed rejection, the unknown-Turn no-op,
  a blocking-dispatch independent-cancellation boundary, stale prepared-snapshot
  prevention, accepted-D-6-before-dispatch handling, pre-cancellation terminal
  reservation, terminal-commit-in-flight reconciliation, atomic interruption
  snapshots, post-cancellation fencing retention, a sealed Turn's distinct
  dispatch-rejection interruption code, and unavailable-adapter
  reconciliation without an early timeout. Reassess the parent
  harness when the T-2 black-box
  `tests/cand_2_cancellation.rs` transport harness lands.
- The repository has configured automatic review evidence on prior pull-request
  revisions. ADR-0003 has no pushed implementation revision yet; exact-revision
  review coverage is therefore pending and remains a later review-ready gate.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

This section is inactive because Decision Status is `Accepted` and
Implementation Status is `In Progress`. When triggered:

- [ ] Move this file to `docs/adr/archive/ADR-0003-default-deny-tool-approval-execution-boundary.md`.
- [ ] Update every governed-source marker and cross-reference to the archived path.
- [ ] If superseded, synchronize reciprocal `Supersedes` / `Superseded By` paths.
- [ ] Retain `Superseded By: None` when no replacement exists.
- [ ] Update this record's single `docs/adr/INDEX.md` row with final status and path.
- [ ] Confirm no live record or code marker still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-14 | Addressed the interruption-diagnosis and test-determinism review with test-first regressions: a sealed Turn's rejected dispatch claim now reports the distinct `ExecutionFailure::InterruptionRequested` code instead of an approval-mismatch diagnosis, and the sealed-claim regression deterministically waits with `recv_timeout` on a post-seal signal from a blocking cancellation service instead of a fixed 50ms sleep, proving the sealed-but-still-Prepared intermediate state before release. Decomposition evidence was re-measured from current source and the zh-CN translation's decomposition review block was re-synchronized. No approved decision content or acceptance status changed. | @kimi |
| 2026-08-14 | Verified the interruption-diagnosis corrections: `cargo fmt --all --check`, strict `koduck-ai` all-target/all-feature Clippy, the 31 focused cancellation regressions, and the complete 211-test `koduck-ai` all-target/all-feature suite pass; governance validator tests and repository governance validation pass. No incomplete acceptance check was promoted to `Pass`. | @kimi |
| 2026-08-14 | Addressed the cancellation convergence review with test-first regressions: `DisabledExecutor` returns a distinct unavailable result; the catalog atomically seals known and unknown interrupted Turns against later D-7 allocation; and a failed pre-dispatch terminal commit retains its already-running reservation so interruption cannot cancel a never-dispatched executor action. Focused cancellation tests moved into responsibility-specific sibling modules so the shared harness remains below the 1,200-line exception limit. Focused and complete Rust tests, formatting, strict Clippy, governance tests, and repository governance validation pass. | @codex |
| 2026-08-13 | Narrowed the bounded-cancellation acknowledgement type with a test-first regression: `CancelAcknowledgement::Acknowledged` now carries a `CancelledEffectState` of only `NotStarted` or `Started`, so an acknowledged cancellation can commit only a defined `cancelled/not_started` or `cancelled/started` terminal. An unacknowledgeable effect is reported `NotAcknowledged` and commits `timed_out/unknown`; `cancelled/unknown` is no longer reachable. Added the `acknowledged_cancellation_only_commits_defined_cancelled_terminals` regression. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @zcode |
| 2026-08-13 | Addressed a cancellation-deadline review finding with a test-first regression: the bounded cancellation path now re-reads the C-5 clock after the executor cancellation returns, so an acknowledgement arriving after the 30-second action deadline commits `timed_out/unknown` instead of `cancelled`. Added the `late_cancellation_acknowledgement_commits_timeout_with_unknown_effect` regression. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @zcode |
| 2026-08-13 | Addressed a post-dispatch terminal-commit review finding with a test-first regression: when an executor effect has been requested (`AfterDispatch`), a conditional canonical terminal write that fails `Unavailable` or `Fenced` now retains the D-7 reservation for reconciliation instead of releasing the running D-7 back to a competing interrupter, which prevents a second cancellation from committing a contradictory terminal. Added the `post_dispatch_durability_failure_keeps_the_running_attempt_reserved_against_interruption` regression. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @zcode |
| 2026-08-13 | Replaced continuously synchronized source-line and method-span evidence with stable fully qualified implementation touchpoints and one decisive code excerpt where the symbol alone was insufficient. Decomposition measurements remain explicitly point-in-time review evidence, and Rust architecture tests no longer compare ADR prose or line counts to current source. No approved decision, scope, or acceptance status changed. | @codex |
| 2026-08-13 | Addressed prepared-cancellation race and expired-before-dispatch review findings with test-first regressions: after a prepared cancellation loses to a concurrent `prepared -> running` transition, C-5 refreshes that exact cataloged D-7 and sends one bounded running cancellation; a zero remaining action budget commits `timed_out/not_started` without an executor dispatch. Re-measured affected source/test evidence. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @codex |
| 2026-08-13 | Addressed terminal-commit and clock-domain review findings with test-first regressions: a `Conflict` result now retains the D-7 reservation because it reports that another canonical terminal already won, keeping the mirror unavailable until reconciliation; C-5 timestamps are converted to a relative remaining 30-second action budget before executor dispatch or cancellation, so the executor applies the budget in its own monotonic clock domain. Re-measured affected source/test evidence. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @codex |
| 2026-08-13 | Addressed two cancellation/terminal-commit review findings with test-first regressions: when a canonical terminal has won but the local D-7 mirror cannot be updated, its authority reservation remains held until reconciliation rather than reopening dispatch or cancellation; the blocked-dispatch interruption regression now uses an independent real cancellation coordinator and proves one executor cancellation plus one durable terminal commit before dispatch is released. Re-measured affected source/test evidence. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @codex |
| 2026-08-13 | Addressed cancellation review findings: interruption conditionally closes the exact requested D-6 through a required pending-approval port before closing its prepared D-7, enumerates and closes every cataloged live D-7 in one interruption, revalidates the lease before a prepared cancellation commit, and supplies a bounded 30-second action budget to both executor dispatch and cancellation. New regressions cover exact D-6 closure, multiple prepared D-7s, prepared fencing, and deadline propagation. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @codex |
| 2026-08-12 | Drafted project Full ADR, selected ADD-0001 CAND-2, and defined the default-deny exact-attempt Tool/MCP approval and isolated-execution contract. | @codex |
| 2026-08-12 | Addressed pre-approval review precision findings: exercised both approval-expiry legs, made AC-12 executable, made the audit cap normative, scoped duplicate-dispatch wording per D-7, charged retries to the attempt budget, and aligned adapter-dependent checks with T-2. | @codex |
| 2026-08-12 | Accepted after the human approver self-declared `@linhai` in the active ADR-0003 context and supplied exact `Approve`; recorded Approval Time `2026-08-12T17:56:05+08:00`. Implementation Status remains `Not Started`; no Approval Context Revision is recorded because the approved uncommitted revision has no immutable commit yet. | @linhai |
| 2026-08-12 | Corrected Completion Checklist A-7 from `Complete` to `Not Started` because its five Risk Coverage Matrix rows require runtime `Pass` evidence before review-ready or completion; this evidence-status correction changes no approved decision content. | @codex |
| 2026-08-12 | Entered Implementation Status `In Progress` and started test-first T-1 implementation after ADR acceptance. | @codex |
| 2026-08-12 | Completed initial TDD cycles for default-deny policy, exact D-6/D-7 authorization, earlier-deadline expiry, attempt budgeting, lease-fenced result commitment, and an explicitly disabled production executor. Focused tests and strict Clippy pass; full-suite verification is recorded separately after the final routed run. T-1/T-2 remain `In Progress` because retry/cancellation, authenticated approval transport, projections, provider integration, and persistence are not complete. | @codex |
| 2026-08-12 | Verified the initial implementation increment: `cargo fmt --all --check`, strict all-target/all-feature Clippy, and all 115 tests pass. The complete suite ran with permitted loopback binding because two existing provider-timeout tests cannot bind listeners inside the filesystem sandbox. This evidence does not complete any ADR check whose remaining production boundary is not implemented. | @codex |
| 2026-08-12 | Addressed implementation review findings with test-first regressions: the coordinator now consumes a canonical D-7 dispatch claim, D-7 preparation consumes a Turn attempt slot, executor failures and successes preserve effect-state evidence, identical terminal approval replay remains idempotent after expiry time, and the no-fallback check scans the complete production graph plus manifest. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-12 | Verified the review corrections: `cargo fmt --all --check`, strict all-target/all-feature Clippy, the focused approval/execution/architecture tests, and the complete 119-test `koduck-ai` all-target/all-feature suite pass. No acceptance check with an incomplete production precondition was promoted to `Pass`. | @codex |
| 2026-08-12 | Addressed the second implementation review with test-first regressions: guarded terminal transitions prevent stale replay rewrites; success and output require conditional durable commit; a non-cloneable Turn authority owns attempt allocation and rejects duplicate D-7 identities; an opaque permit prevents direct executor calls; and versioned canonical encoding now produces a fixed-vector SHA-256 action digest. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-12 | Addressed the third implementation review with test-first regressions: target-scoped profiles now retain exact ID/version, adapter-validated JSON and descriptor schemas fail closed before policy authorization, mutable D-6/D-7 authorities cannot clone, reconstructed process-local Turn handles share attempt budget and running arbitration, and fencing or storage failure returns reconciliation-pending instead of an uncommitted terminal. T-1/T-2 remain `In Progress`; cross-instance authority remains owned by T-3 PostgreSQL work. | @codex |
| 2026-08-12 | Verified the third review corrections: formatting, strict all-target/all-feature Clippy, focused red/green regressions, and the complete 137-test `koduck-ai` all-target/all-feature suite pass. The full suite used permitted loopback binding because two existing provider-timeout tests cannot bind listeners inside the filesystem sandbox. Unsupported JSON Schema constraints fail closed, and no incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-12 | Verified the second review corrections: formatting, strict all-target/all-feature Clippy, all 35 focused CAND-2/architecture tests, and the complete 125-test `koduck-ai` all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the fourth implementation review with test-first regressions: an explicitly injected strong Turn registry preserves budget and attempt identity after all handles drop and supports terminal-Turn cleanup; raw serialized action input is capped before JSON parsing; executor output is bounded during incremental construction and overflow poisons completion; and fenced durable commits distinguish pre-dispatch from post-dispatch reconciliation. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-13 | Verified the fourth review corrections: formatting, strict all-target/all-feature Clippy, all 52 focused CAND-2/architecture tests, and the complete 142-test `koduck-ai` all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the fifth implementation review with test-first regressions: D-7 preparation now has one public lease-validating entry and fenced preparation consumes no attempt slot; the unbound terminal-Turn cleanup API was removed; JSON decimals retain their exact arbitrary-precision text; and the decomposition review was remeasured for both source files above 400 lines. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-13 | Verified the fifth review corrections: formatting, strict all-target/all-feature Clippy, all 53 focused CAND-2/architecture tests, and the complete 143-test `koduck-ai` all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the sixth implementation review with test-first regressions: replaced the externally constructible multi-Turn registry with a lease-initialized Turn-scoped preparation owner; stale bindings cannot seed profile identity; authority lookup is no longer public; all handles for the Turn retain one shared budget/running state; and state lifetime is bounded by the preparer plus returned handles. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-13 | Verified the sixth review corrections: formatting, strict all-target/all-feature Clippy, all 54 focused CAND-2/architecture tests, and the complete 144-test `koduck-ai` all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the seventh implementation review with test-first regressions: C-5 seals bindings before D-6/D-7 creation; D-6 decisions require typed same-tenant, same-Thread approval scope; an injected runtime shares one Turn authority across preparers with weak lifecycle cleanup; conditional commits distinguish won, existing, and conflicting terminals; duplicate JSON Schema members fail closed; and concurrent attempts retain their exact terminal code. The authorized no-D-6 `read_data` path remains executable. T-1/T-2 remain `In Progress`. | @codex |
| 2026-08-13 | Verified the seventh review corrections: formatting, strict all-target/all-feature Clippy, all 62 focused CAND-2/architecture tests, and the complete 152-test `koduck-ai` all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the eighth implementation review with test-first regressions: C-5 binding authority now comes only from an injected configuration-backed sealing service; approval decisions require an injected C-7 authorizer plus independent tenant/Thread checks; an explicit strong process-local authority store prevents runtime-handle and temporary-drop resets; and reconstructed canonical terminals carry exact D-7 binding/version while rejecting oversized or mismatched results. T-1/T-2 remain `In Progress`; PostgreSQL authority and terminal reclamation remain T-3 work. | @codex |
| 2026-08-13 | Verified the eighth review corrections: formatting, strict all-target/all-feature Clippy, all 69 focused CAND-2/architecture tests, and the complete 159-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the ninth implementation review with a failing architecture regression followed by the minimum authority-boundary change: configuration-backed C-5 sealing and C-7 authorization/service construction are crate-owned instead of public extension points, and every process runtime handle resolves one process-owned Turn authority root rather than accepting a caller-constructed store. Authority-dependent tests moved under `tests/internal/` and remain crate unit tests so private production authority is not reopened for test convenience; the AC-4 command and affected-path evidence were updated only as approval-preserving path maintenance. T-1/T-2 remain `In Progress`; T-2 runtime wiring and T-3 PostgreSQL authority/terminal reclamation remain incomplete. | @codex |
| 2026-08-13 | Verified the ninth review corrections: `git diff --check`, formatting, strict workspace all-target/all-feature Clippy, the focused authority/architecture and AC-2/AC-4 commands, and the complete 159-test workspace all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Implemented the T-1 cancellation and timeout contract: the isolated-executor port gained one bounded cancellation behind an opaque permit, running-D-7 interruption validates the current lease and commits the acknowledged effect state or `timed_out/unknown` when unacknowledged, every executor response is deadline-checked against the 30-second action bound so late completions commit `timed_out` with no output delivery, the interruption handle resolves the cataloged live D-7 through the shared process authority, and `DisabledExecutor` answers cancellations fail-closed. Focused `cand_2_cancellation_tests` fixtures cover TC-09/TC-10 logic; decomposition-review evidence was remeasured. T-1 remains `In Progress` pending production wiring and runner integration. | @zcode |
| 2026-08-13 | Verified the cancellation and timeout increment: `cargo fmt --all --check`, strict all-target/all-feature Clippy, and the complete 192-test `koduck-ai` all-target/all-feature suite pass, including the nine new `cand_2_cancellation_tests` fixtures and the updated ADR-evidence architecture check. No incomplete production-boundary acceptance check was promoted to `Pass`. | @zcode |
| 2026-08-13 | Addressed the tenth implementation review with three failing regression cycles: executor success and error results now share the required pre-commit lease check; the owned schema constructor rejects duplicate properties instead of last-value overwrite; and the global `OnceLock` was replaced with an explicitly injected crate-owned runtime root. Process-local Turn state can be reclaimed only for a canonical terminal Turn after all D-7 records are terminal and every preparer/attempt/authority handle is gone; active or live authority fails closed. Multi-Turn lookup/reclamation moved into a focused 114-line domain submodule so the materially changed aggregate remains below the production-source exception limit. T-1/T-2 remain `In Progress`; production root/reclamation wiring and T-3 PostgreSQL authority remain incomplete. | @codex |
| 2026-08-13 | Verified the tenth review corrections: `git diff --check`, formatting, strict workspace all-target/all-feature Clippy, focused schema/fencing/reclamation/architecture tests, and the complete 163-test workspace all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the eleventh implementation review with three failing regression cycles: recursive duplicate action-parameter members now fail closed before canonicalization; a canonical terminal that cannot update the local D-7 mirror now returns typed reconciliation instead of a fabricated failure; and process-local Turn reclamation is removed until T-3 can supply canonical terminal proof and prevent budget resurrection. The current strong authority root deliberately retains Turn state across temporary handle loss. T-1/T-2 remain `In Progress`; T-3 persistence and safe reclamation remain incomplete. | @codex |
| 2026-08-13 | Verified the eleventh review corrections: `git diff --check`, formatting, strict workspace all-target/all-feature Clippy, the three focused duplicate-input/terminal-conflict/authority-boundary regressions, and the complete 163-test workspace all-target/all-feature suite pass. No incomplete production-boundary acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the twelfth implementation review with a failing evidence-consistency regression: T-1/T-2 evidence now distinguishes source/test seams from incomplete production runtime wiring, removes the stale reclamation claim, and records explicit decomposition reviews for the 602-line Tool domain file and 81-line execution coordinator method, including the configured-complexity-tool N/A and substitute nesting review. No approved decision content or acceptance status changed. | @codex |
| 2026-08-13 | Verified the twelfth review corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, the focused ADR evidence-consistency regression, and the complete 164-test `koduck-ai` all-target/all-feature suite pass. The architecture test file remains below its 600-line review threshold, and no incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the thirteenth implementation review with a test-first dispatch-rejection regression: canonical start rejection now uses non-final `ExecutionPending::DispatchRejected` instead of an uncommitted `ToolExecutionOutcome`; decomposition evidence is derived from current source measurements; and each Not Started Risk Coverage row now records its concrete missing acceptance prerequisites. No approved decision content or acceptance status changed. | @codex |
| 2026-08-13 | Verified the thirteenth review corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, all 26 focused execution tests, the source-derived ADR evidence-consistency test, and the complete 164-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests; no incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the fourteenth implementation review with failing architecture regressions: the raw D-7 dispatch claim transition is crate-internal and the architecture guard prohibits reopening it publicly; the decomposition evidence test now derives every recorded production/test file count plus the coordinator-method span from current source, and the ADR measurements were synchronized. No approved decision content or acceptance status changed. | @codex |
| 2026-08-13 | Verified the fourteenth review corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, both focused architecture regressions, and the complete 164-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests; the architecture test file remains below its 600-line review threshold and no incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the fifteenth implementation review with test-first regressions: untrusted descriptor Schema text is capped at 65,536 bytes before either deserialization pass with exact at/over-limit evidence, and uniquely named crate-internal D-7 claim/mirror transitions are guarded by whole-production-source call-site counts so no additional bypass can be added silently. Decomposition evidence was synchronized without changing approved decision scope or acceptance status. | @codex |
| 2026-08-13 | Verified the fifteenth review corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, focused schema-limit/call-site/ADR-evidence regressions, all 15 approval and 26 execution tests, and the complete 165-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests; the architecture test file remains below its 600-line review threshold and no incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Implemented the T-1 retry deliverable and addressed the following review rounds: a crate-internal `ToolExecutionDriver` runs authorize, prepare, approve-or-cancel, and dispatch with exactly one proven-pre-effect retry (fresh D-7 identity, re-evaluated policy, fresh D-6), maps a budget-exhausted retry to `failed/attempt_limit`, cancels the prepared D-7 without dispatch for a declined/cancelled/expired D-6, re-reads a controlled clock at each D-6 creation and dispatch with the D-7 start time clamped to never precede the verified decision time, and delivers no stale committed result when the owner is fenced during retry preparation. Twelve focused retry fixtures cover the contract; decomposition reviews were recorded for the 98-line driver method, the 744-line retry harness, and the architecture test file, which now measures 675 lines — above its 600-line review threshold, superseding the prior below-threshold verification note. Verification: formatting, strict all-target/all-feature Clippy, and the complete 183-test `koduck-ai` suite pass; AC-9 end-to-end retry verification remains a T-2 black-box deliverable and no incomplete acceptance check was promoted to `Pass`. | @zcode |
| 2026-08-13 | Addressed cancellation concurrency review findings with three test-first regressions: interruption now consumes an independently supplied cancellation service, so a blocking dispatch coordinator cannot prevent authenticated cancellation from reaching its executor path; authority records reserve a live D-7 terminal transition before its conditional durable write, preventing a stale prepared snapshot from committing `cancelled/not_started` after dispatch; and an already accepted D-6 no longer prevents interruption from closing its still-prepared exact D-7. The cancellation service revalidates the lease after acknowledgement before it commits. T-1/T-2 remain `In Progress`; runtime assembly and authenticated transport remain T-2 work, and no acceptance check was promoted. | @codex |
| 2026-08-13 | Verified the cancellation concurrency corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, the 15 focused cancellation regressions, the ADR-evidence architecture regression, and the complete 198-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the existing provider-timeout tests. No incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Addressed the follow-up cancellation concurrency review with two test-first regressions: interruption takes one authority-lock snapshot, so any in-flight live D-7 returns `ReconciliationRequired{terminal_conflict, unknown}` before a visible peer is partly closed; and a running-D-7 cancellation atomically reserves its exact terminal before the executor cancel side effect, retaining that reservation through post-cancel fencing or durable-write failure so a second interrupter cannot send a duplicate cancellation. T-1/T-2 remain `In Progress`; no acceptance check was promoted. | @codex |
| 2026-08-13 | Verified the atomic-interruption and post-cancellation reconciliation corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, the 21 focused cancellation regressions, the ADR-evidence architecture check, and the complete 187-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests. No incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Verified the follow-up cancellation concurrency corrections: `git diff --check`, formatting, strict `koduck-ai` all-target/all-feature Clippy, both new focused cancellation regressions, the ADR-evidence architecture check, and the complete 185-test `koduck-ai` all-target/all-feature suite pass. The complete suite used permitted loopback binding for the two existing provider-timeout tests. No incomplete acceptance check was promoted to `Pass`. | @codex |
| 2026-08-13 | Corrected decomposition-review evidence after the cancellation follow-up: the shared terminal module is 113 physical lines, and the 85-line interruption, 78-line running-cancellation, and 86-line reserved-terminal-commit methods now record their explicit cohesion and nesting reviews. The architecture evidence check derives these measurements from current source. No approved decision content or acceptance status changed. | @codex |
