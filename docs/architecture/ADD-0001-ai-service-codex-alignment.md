# ADD-0001: AI Service Boundary and Codex Alignment

## Metadata [Required]

- **Design Status**: Current
- **Date**: 2026-08-10
- **Author**: Codex
- **Architecture Owner**: @linhai
- **Required Approver**: @linhai
- **Approver [Conditionally Required — Design Status is or has been `Current`]**: @linhai
- **Approval Time [Conditionally Required — Design Status is or has been `Current`]**: 2026-08-11T10:37:34+08:00
- **Approval Evidence [Conditionally Required — Design Status is or has been `Current`]**: Approve
- **Retired By [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: N/A — Design Status is `Current`; the document has not been retired
- **Retirement Time [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: N/A — Design Status is `Current`; the document has not been retired
- **Retirement Evidence [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: N/A — Design Status is `Current`; the document has not been retired
- **Retirement Reason [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: N/A — Design Status is `Current`; the document has not been retired
- **Scope Level**: Repository / Cross-project
- **Scope**: The future Koduck AI runtime and its contracts with API clients, model providers, authentication, memory, tool execution, background work, and extension providers
- **Trello Sources**: [Koduck card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)
- **Figma Sources [Conditionally Required — UI is in scope]**: N/A — this design covers service and protocol boundaries and does not change a Web or native UI
- **Related**: [Chinese translation](translations/zh-CN/ADD-0001-ai-service-codex-alignment.md); [Koduck predecessor baseline](https://github.com/hailingu/koduck-quant/tree/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai); [OpenAI Codex reference baseline](https://github.com/openai/codex/tree/3c60d4da648bfa98e3c51c5161ac2720519c733e)
- **Supersedes [Conditionally Required — this ADD replaces another]**: None
- **Superseded By [Conditionally Required — this ADD is replaced]**: None

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

## Context And Solution Summary [Required]

Koduck is a from-scratch successor to `koduck-quant`. The new repository
currently contains governance scaffolding and no services. The predecessor's
`koduck-ai`, fixed at commit `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe`, is
functional research evidence only: its infrastructure has been removed, it is
not an operating baseline, and its contracts are not compatibility or rollback
requirements. It is a Rust AI gateway and
orchestrator with REST/SSE APIs, provider adapters, a native tool-use loop,
MCP clients, agent profiles, skills, memory and multitask clients, background
workers, authentication, and reliability policy in one crate. The baseline has
161 tracked Rust source files; multiple orchestration, provider, configuration,
and worker files exceed 800 physical lines, including a 2,073-line native tool
loop. These measurements are review signals, not a conclusion that mechanical
file splitting is the target architecture.

OpenAI Codex is used as a public reference, fixed at commit
`3c60d4da648bfa98e3c51c5161ac2720519c733e`. Its relevant architectural ideas
are a provider-independent core, explicit thread/turn/item lifecycle, a typed
application protocol, replaceable thread storage, distinct execution and
sandbox responsibilities, explicit approval requests, and separately loaded
MCP, skill, plugin, and repository-instruction capabilities. Codex is not a
product specification for Koduck and its code layout is not a migration plan.

**Solution summary**: Build the future Koduck AI capability around an owned,
provider-independent agent core and a versioned thread/turn/item domain model.
Define new northbound REST/SSE and owned persistence contracts from this target
model; use predecessor behavior only to identify functional scenarios. Move capability discovery, policy
evaluation, approval, and execution behind explicit ports; require privileged
execution to run through a least-privilege execution boundary. Treat model
providers, storage, MCP, skills, repository instructions, and presentation
protocols as independently replaceable adapters. Preserve the intended
multi-provider, tenant, semantic-memory, and background-task capabilities
rather than adopting Codex's product-specific local persistence and auth model
or inheriting removed predecessor infrastructure.

**Greenfield operating model**: No predecessor deployment, APISIX route, shared
history, or fallback path exists. Each candidate defines and verifies the new
contract it owns before first promotion. A failed candidate is not promoted;
its source or artifact is reverted or quarantined without attempting route-back
to removed infrastructure. After the first verified Koduck AI release exists,
any deployment rollback must target a verified new artifact under a separate
accepted OCR.

**Design boundary**: This ADD defines solution capabilities, logical data,
component responsibilities, flows, constraints, and ordered ADR candidates. It
does not authorize implementation, select source files or crates, define a
physical schema, freeze wire-field definitions, add dependencies, or prescribe
executable build, test, deployment, or rollback commands.

## Requirement Baseline [Required]

| ID | Trello source | Requirement baseline | Acceptance outcome | Priority and constraints | Last checked |
| --- | --- | --- | --- | --- | --- |
| R-1 | [Card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87) | Research the current AI-service baseline against public OpenAI Codex and establish an auditable target boundary and migration direction. | A traceable gap matrix, adoption decisions with reasons, external-contract and security boundaries, dependency-ordered migration slices with validation and rollback boundaries, and a project Full ADR proposal after ADD approval. | Highest board position. No source, configuration, dependency, build, release, or deployment work before an eligible approver accepts the governing ADR. Trello is coordination context, not decision authority. | 2026-08-10 |
| R-2 | N/A — user instruction in the active Codex task, not a Trello card | Provide an additional Chinese version of the ADD. | A complete Chinese translation exists and identifies the English ADD as authoritative. | The translation must not create a second decision identity or diverge from the indexed ADD. | 2026-08-10 |

## Goals And Non-Goals [Required]

Goals:

- Establish one auditable predecessor functional-research baseline and one immutable Codex reference baseline.
- Define which Codex concepts Koduck adopts, adopts with adjustment, or does not adopt.
- Separate orchestration policy from transport, provider, storage, extension, and privileged-execution concerns.
- Preserve the predecessor's functional intent as research scenarios while defining new owned Koduck contracts without wire-parity or runtime-fallback obligations.
- Define least-privilege, approval, isolation, audit, cancellation, and recovery boundaries before tool execution expands.
- Provide ordered, independently reviewable ADR candidates with binary architecture-level acceptance context.
- Provide a synchronized Chinese translation for reviewers who prefer Chinese.

Non-goals:

- Forking OpenAI Codex or promising feature parity with its CLI, desktop app, cloud product, or UI.
- Reusing Codex's ChatGPT-specific authentication, account, rate-limit, or model-catalog behavior.
- Making local JSONL or SQLite state the canonical Koduck conversation or memory store.
- Selecting a physical crate layout, wire schema, database schema, dependency set, or implementation framework.
- Changing current APIs, deployments, runtime configuration, or service ownership in this ADD.
- Implementing proactive multi-agent orchestration before the single-agent lifecycle and execution policy are proven.

## Functional Capability Design [Required]

| ID | Actor | Trigger | Capability and outcome | Business rules and edge cases | Requirements |
| --- | --- | --- | --- | --- | --- |
| F-1 | API client or presentation adapter | A user starts, resumes, forks, steers, interrupts, or reads work | Represent work as a stable thread containing ordered turns and typed items, with explicit active and terminal states. | Resume always starts a new turn on the same thread from canonical history; it never reactivates a terminal turn. An authenticated client interrupt produces `interrupted`; a platform, policy, or dependency stop produces `cancelled`. Both are terminal. New versioned REST/SSE behavior is defined by owned contracts. | R-1 |
| F-2 | Agent core | A turn is accepted | Assemble instructions, context, available capabilities, model input, and policy into one observable orchestration lifecycle. | Provider-specific types do not enter the domain model; state transitions have one owner; partial results and terminal errors remain distinguishable. | R-1 |
| F-3 | Model adapter | The core requests inference | Translate owned model input and streamed output to one configured provider without leaking provider wire types into the core. | CAND-1 has no provider fallback; time, token, retry, and output budgets are bounded; usage and terminal status are preserved. Any later automatic provider fallback requires a separate Accepted ADR. | R-1 |
| F-4 | Tool or MCP provider | The core needs capability discovery or invocation | Discover typed tools, validate requests, evaluate policy, request approval when required, and dispatch through an execution boundary. | Untrusted descriptions and results never grant authority; default deny applies to unknown privileged effects; approval is bound to the exact action and scope. | R-1 |
| F-5 | Thread-store adapter | Thread state changes | Persist canonical thread/turn/item history and metadata through an owned store port backed by an AI-owned durable store. | One canonical owner per datum; appends are ordered and idempotent; local caches are reconstructable and never silently become truth. Semantic Memory and background Multitask integrations do not own canonical turn history. | R-1 |
| F-6 | Extension owner | Instructions, skills, plugins, or MCP capabilities change | Load validated, provenance-bearing extension metadata without changing core orchestration code. | Precedence is deterministic; invalid extensions fail visibly; tenant and thread isolation is preserved; extensions cannot widen permissions by declaration. | R-1 |
| F-7 | Operator or reviewer | A privileged action, failure, or recovery occurs | Observe structured lifecycle, policy, approval, execution, and recovery evidence without exposing secrets or sensitive prompt content. | Content logging is minimized and redacted; correlation IDs connect events; audit evidence distinguishes request, decision, attempt, and result. | R-1 |
| F-8 | Chinese-speaking reviewer | The ADD is reviewed | Read a synchronized Chinese translation while retaining one English authoritative identity. | Status, IDs, evidence baselines, candidates, and normative meaning must match the English ADD; conflicts resolve to the English ADD. | R-2 |

## Data Model Design [Conditionally Required — data is created, updated, deleted, transferred, retained, or changes ownership, classification, lifecycle, relationships, or invariants]

### Entities And Lifecycle

| ID | Entity | Purpose | Ownership | Classification | Lifecycle |
| --- | --- | --- | --- | --- | --- |
| D-1 | Thread | Stable container for one user-visible body of work and its lineage. | Koduck AI thread-store domain; durable data is provided by its approved AI-owned store adapter. | Tenant/user content and metadata; potentially sensitive. | Created, optionally forked, active, archived, or deleted under owner policy. Thread deletion does not silently cascade to separately retained D-6/D-7 security evidence. |
| D-2 | Turn | One accepted input-to-terminal-outcome execution attempt within a thread. | Agent core for live state and foreground-liveness reconciliation; thread store for durable history and fenced liveness leases. | Potentially sensitive prompts, context references, and policy metadata. | Queued or started, with `recovery-pending` as a nonterminal started substate when durability is unavailable, then completed, interrupted, failed, or cancelled. Every foreground started turn has a C-2-owned heartbeat on a C-6-persisted lease generation. After the deterministic liveness window expires, a healthy C-2 reconciler fences the lost owner and appends `cancelled`; if C-6 was unavailable, reconciliation occurs when it returns. A terminal turn never returns to active. `Interrupted` means an authenticated client stopped it; `cancelled` means the platform, policy, or a dependency stopped it. Resume creates a new turn. |
| D-3 | Item | Ordered typed unit within a turn, such as input, reasoning summary, tool call, approval-status projection, tool result, file change, or agent message. | Agent core creates domain items; thread store persists them; presentation adapters project them. D-6, not D-3, owns approval authority. | Classification follows payload; tool and model output is untrusted. An approval projection carries the D-6 identity and status but no independent authority. | Appended with stable identity and order. A correction is a new versioned item that references the prior item; the prior item is never mutated or removed. |
| D-4 | Capability Descriptor | Validated description and schema for a tool, MCP capability, skill, or plugin, including effect classification, idempotency, retry safety, and applicable deadline/output constraints. | Extension/tool registry. | Public or internal metadata; descriptions and self-declared execution properties remain untrusted until validated by policy. | Discovered, validated, enabled, refreshed, disabled, or withdrawn with provenance and a stable version. |
| D-5 | Permission Profile | Named limits for filesystem, network, process, data, and service access. | Security policy domain. | Security-sensitive policy, not a secret. | Defined and versioned, then selected and optionally narrowed for a turn. Approval never mutates or widens the profile; when policy permits an approval path, D-6 authorizes only its exact D-7 attempt. |
| D-6 | Approval Request | Canonical security record for one exact proposed privileged action, target, parameters, effect, requested scope, rationale, and decision. | Approval/policy domain through C-5; thread items are projections only. | Security audit data; may contain sensitive paths or command metadata. | Requested, accepted for one exact bounded execution attempt, declined, cancelled, or expired, then linked to that attempt's result if accepted. Reusable session/turn grants are outside this ADD and require a future Accepted ADR. |
| D-7 | Execution Attempt | One bounded tool or process invocation and its observable result. | Execution boundary. | Potentially sensitive input/output and diagnostics. | Prepared, policy-checked, optionally approved, running, then succeeded, failed, timed out, or cancelled. A foreground attempt references the current D-2 lease generation; C-5 rejects dispatch or result commitment after that generation is fenced. |
| D-8 | Extension Manifest | Provenance, declared capabilities, configuration needs, and compatibility information for an extension. | Extension registry. | Internal configuration metadata; secrets referenced but never embedded. | Discovered, validated, activated, updated, disabled, or rejected. |

### Relationships And Invariants

| Relationship | Cardinality and meaning | Invariant |
| --- | --- | --- |
| Thread contains Turn | One thread contains zero or more ordered turns. | A turn belongs to exactly one thread; its thread identity does not change. |
| Turn contains Item | One turn contains one or more ordered items, including any later correction item. | For the same canonical snapshot/version, replay produces the same externally observable sequence. A correction appends a new ordered item that references its predecessor; it never rewrites prior replay history. |
| Thread forks Thread | A thread may have one parent and zero or more children. | Fork lineage is immutable and cross-tenant lineage is prohibited. |
| Foreground Turn holds Liveness Lease | Each foreground started turn has one current C-6-persisted lease generation renewed by its C-2 owner. | Only the current generation may append or dispatch/commit a foreground D-7 attempt. Missing heartbeats beyond the deterministic liveness window fence the old owner; concurrent reconcilers use one conditional, idempotent terminal transition keyed by turn and lease generation, producing exactly one `cancelled`. An expired owner can never resume or overwrite that result. |
| Turn selects Permission Profile | Each turn resolves exactly one effective profile. | Later extension, model output, or approval cannot mutate or widen the resolved profile; an accepted D-6 remains a one-attempt authorization evaluated against policy. |
| Approval Request projects to Item | One D-6 record may produce status-projection items in its owning turn. | D-6 is canonical; a D-3 projection references the exact D-6 version, cannot authorize execution, and is corrected only by appending a later projection. |
| Approval Request authorizes Execution Attempt | One accepted D-6 approval authorizes exactly one bounded D-7 execution attempt with the same action, target, parameters, effect, and scope. | Any parameter, target, effect, scope, or attempt-identity drift requires a new policy evaluation and a new approval. No session/turn-wide reusable grant exists in this design. |
| Capability Descriptor produces Execution Attempt | An attempt references one validated descriptor version. | Execution is rejected if the descriptor is missing, stale beyond policy, disabled, or incompatible. |
| Extension Manifest exposes Capability Descriptor | One manifest may expose many descriptors. | Removing or disabling a manifest makes its descriptors unavailable without rewriting history. |

## Architecture Design [Required]

| ID | Component or dependency | Responsibility | Conceptual inputs and outputs | Dependencies | Accepted constraints |
| --- | --- | --- | --- | --- | --- |
| C-1 | Presentation boundary | Expose the new versioned REST/SSE contract, authenticated approval protocol, and future typed application protocols; translate them to owned domain requests, approval decisions, and events. | Client and approver requests, gateway context, thread/turn operations, approval decisions; typed lifecycle events and owned REST/SSE responses. | C-2, C-5, C-7. | C-1 delegates signed-claim validation and trust-context construction to C-7; it never validates identity by itself. The new contract is authoritative; predecessor wire parity is not required. UI design is out of scope. |
| C-2 | Agent core | Own thread/turn orchestration, state transitions, context assembly, budgets, cancellation, foreground lease heartbeats, orphan reconciliation, and provider-independent policy flow. | Owned turn input, instructions, context references, capabilities, lease-expiry signals; typed items and terminal outcomes. | C-3, C-4, C-5, C-6, C-7. | Any healthy C-2 instance may reconcile an expired foreground lease, but only through C-6 generation fencing. No provider wire types, Web handlers, database types, or privileged host execution enter the core. |
| C-3 | Provider adapters | Translate owned inference requests and streams for OpenAI-compatible and other configured providers. | Model-neutral messages, tool schemas, budgets; model events, usage, and normalized errors. | External provider APIs. | Provider selection is explicit; the initial baseline has no automatic fallback. Secrets remain in adapter configuration boundaries. |
| C-4 | Capability and extension registry | Load repository instructions, agent profiles, skills, plugins, and MCP/tool descriptors with precedence and provenance. | Configured roots and remote catalogs; validated descriptors and diagnostics. | MCP/tool providers, configuration. | Metadata is untrusted; extension declarations never grant execution authority. |
| C-5 | Policy, approval, and execution boundary | Resolve permission profiles, evaluate effects, own canonical D-6 approval records, bind each accepted approval to one D-7 attempt, validate the current foreground lease generation through C-6, and dispatch through sandboxed or isolated executors. | Proposed action, immutable trust context, and applicable turn lease generation; authenticated approval decision delivered through C-1; policy decision, D-6 status, execution events and result returned through C-2. | C-6, Tool service, MCP providers, platform sandbox or isolated worker. | C-5 exposes owned ports and does not depend on C-1. It rejects dispatch and result commitment from a fenced foreground owner. Default deny, cancellation, timeout, output cap, and audit are mandatory; C-2 never performs direct host execution, and no reusable session/turn approval exists. |
| C-6 | Thread-store port and AI-owned durable adapter | Persist and retrieve canonical history, metadata, lineage, foreground liveness leases, checkpoints, and idempotency state in a shared durable store owned by the AI service boundary. | Thread/turn/item appends and queries, lease acquire/renew/expire operations; ordered history, metadata, and fenced lease generations. | AI-owned PostgreSQL datastore; later semantic Memory and background Multitask adapters consume owned projections or commands. | Lease expiry and orphan terminal transition are conditional and idempotent by turn plus generation; a stale generation cannot append after fencing. The AI-owned store is canonical for Thread/Turn/Item; process-local state is reconstructable only. |
| C-7 | Identity and trust-context adapter | Validate gateway/JWT identity and construct immutable tenant/user/thread trust context. | Credentials and gateway context; validated principal and scopes. | APISIX and Auth/JWKS. | Headers cannot replace missing signed claims; secrets and raw credentials do not enter history or logs. |
| C-8 | Observability and audit boundary | Emit structured lifecycle, provider, policy, approval, execution, retry, and recovery signals. | Correlated events from C-1 through C-7; redacted logs, metrics, traces, and evidence references. | Logging, metrics, tracing backends. | Content minimized by default; sensitive content requires explicit environment-safe diagnostics and redaction. |

The table's Dependencies column defines implementation dependency direction.
Return events and responses in the diagram below do not reverse that direction:
C-5 returns owned policy/approval/execution events through C-2, while C-1 is an
adapter that invokes the C-5 decision port after C-7 validation.

### Mermaid Architecture Diagram [Required]

```mermaid
flowchart LR
  subgraph Clients ["Client and gateway boundary"]
    Client["API clients"]
    Approver["Human approver"]
    Gateway["APISIX / Auth / JWKS"]
  end
  subgraph Runtime ["Koduck AI runtime boundary"]
    C1["C-1 Presentation boundary"]
    C7["C-7 Identity and trust-context adapter"]
    C2["C-2 Agent core"]
    C4["C-4 Capability and extension registry"]
    C5["C-5 Policy, approval, and execution boundary"]
    C3["C-3 Provider adapters"]
    C6["C-6 Thread-store port and AI-owned durable adapter"]
    C8["C-8 Observability and audit boundary"]
  end
  subgraph External ["External systems and isolated execution"]
    Providers["Model providers"]
    Extensions["Instructions / profiles / skills / plugins / MCP and tool catalogs"]
    Executor["Sandboxed or isolated executors / Tool service"]
    Stores["AI-owned PostgreSQL datastore"]
    MemoryJobs["Semantic Memory / background Multitask"]
    Telemetry["Logs / metrics / traces / audit evidence"]
  end

  Client -->|"REST / SSE or typed lifecycle operations"| C1
  Approver -->|"Authenticated approval decision"| C1
  Gateway -->|"Validated credentials and gateway context"| C7
  C7 -->|"Immutable tenant / user / thread trust context"| C1
  C7 -->|"Validated principal and scopes"| C2
  C1 -->|"Owned thread / turn requests"| C2
  C1 -->|"Validated exact approval decision"| C5
  C2 -->|"Model-neutral inference and budgets"| C3
  C3 -->|"Provider-native requests"| Providers
  Providers -->|"Stream events / usage / errors"| C3
  C3 -->|"Typed model events"| C2
  Extensions -->|"Untrusted manifests and descriptors"| C4
  C4 -->|"Validated snapshot with provenance"| C2
  C2 -->|"Proposed capability action and current lease generation"| C5
  C5 -->|"Validate foreground lease generation"| C6
  C5 -->|"Bounded action after policy and approval"| Executor
  Executor -->|"Untrusted execution result"| C5
  C5 -->|"D-6 projection, attempt result, or non-execution outcome"| C2
  C2 -->|"Ordered history, lineage, fenced liveness leases, and checkpoints"| C6
  C6 -->|"Canonical storage operations"| Stores
  Stores -->|"Durable state, lease expiry, and recovery input"| C6
  C6 -->|"Versioned semantic-memory projections and background-state contracts"| MemoryJobs
  C6 -->|"Durable state and replay"| C2
  C2 -->|"Typed items and terminal outcomes"| C1
  C1 -->|"Owned REST / SSE responses and lifecycle events"| Client
  C1 -.->|"Ingress and projection events"| C8
  C2 -.->|"Lifecycle and budget events"| C8
  C3 -.->|"Provider events"| C8
  C4 -.->|"Discovery diagnostics"| C8
  C5 -.->|"Policy / approval / execution events"| C8
  C6 -.->|"Persistence and recovery events"| C8
  C7 -.->|"Identity validation events"| C8
  C8 -->|"Redacted telemetry and evidence references"| Telemetry
```

## Control Flow Design [Conditionally Required — the solution has multiple steps, branches, retries, asynchronous work, or failure recovery]

| ID | Trigger and precondition | Happy path | Branches and retries | Failure handling | Observable result |
| --- | --- | --- | --- | --- | --- |
| CF-1 | Client starts or continues a turn / identity and thread access are valid | C-1 normalizes input; C-2 durably creates the started turn and input, acquires and renews its fenced foreground lease through C-6, resolves context/capabilities/policy, invokes C-3, and appends every externally visible item and terminal outcome through C-6 before C-1 publishes it. | Provider retry follows a bounded budget for the selected provider; no provider or legacy runtime fallback occurs. An authenticated client interrupt yields `interrupted`; a platform, policy, dependency, or reconciled owner loss yields `cancelled`. | Invalid identity fails before model/tool use. If the initial started-turn/input/lease write fails, no turn is accepted. A later append failure enters `recovery-pending` and is closed `failed` when C-6 returns. A missing heartbeat beyond the deterministic liveness window fences the old generation; healthy C-2 reconcilers race through one conditional idempotency key, so exactly one appends `cancelled`, including after delayed C-6 recovery. | The client sees rejection with no turn, a durable replayable prefix plus `durability-unavailable`, or an eventual durable `cancelled` terminal for an orphan. No expired owner can append or report completion. |
| CF-2 | Model requests a capability / descriptor is active and compatible | C-4 resolves validated effect, idempotency, retry, and budget metadata; C-5 validates the exact action and current foreground lease generation and, when approval is required, creates canonical D-6 state, obtains the decision through C-1/C-7, and binds acceptance to one D-7 attempt. It executes allowed work in isolation and returns any D-6 projection plus the untrusted result to C-2. | Policy may deny, request narrower input, allow without approval, or require approval. Pre-effect work may retry within metadata and budget; once a privileged D-7 starts, any retry is a new attempt requiring current-lease validation, fresh policy evaluation, and, when required, a new approval. | A fenced lease, decline, cancel, or expiry becomes a typed non-execution result; fencing during execution prevents result commitment and records a cancelled/failed attempt according to observed effect state. Descriptor drift restarts evaluation without reusing approval. | Canonical audit evidence links descriptor version, policy, lease generation, D-6 when applicable, D-7 attempt, result, and D-3 projections without treating projections as authority. |
| CF-3 | Thread is resumed or forked / caller has access | C-6 loads canonical ordered history and lineage; C-2 reconstructs model context under current budget. Resume starts a new turn on the same thread; fork creates a child thread and then a new turn. | Missing optional cache is rebuilt; incompatible historical item versions are translated by a versioned adapter. Foreground orphan closure belongs to CF-1; process recovery of a still-active background turn belongs to CF-5. Neither is Resume. | Corrupt or incomplete canonical history fails visibly and does not create a silently truncated turn; no terminal turn is ever reactivated. | The original terminal turn remains unchanged; the new turn or child thread retains stable lineage and deterministic visible history. |
| CF-4 | Extension inventory changes or a configured source becomes unavailable | When reachable, C-4 discovers, parses, validates, records provenance, and atomically publishes a new capability snapshot. | Invalid entries are excluded with diagnostics. If the source is unavailable, the prior valid snapshot remains active only when an explicit stale policy permits its age and scope; otherwise new resolutions fail closed. | Source loss or load failure never widens permissions and never partially publishes an inconsistent catalog. In-flight turns retain their already resolved snapshot. | New turns observe one coherent fresh or explicitly stale snapshot, or a typed capability-unavailable failure with provenance diagnostics. |
| CF-5 | Background work is accepted / identity, idempotency, and capability policy are valid | C-1/C-2 create durable work intent; Multitask schedules; workers execute the same core lifecycle and C-6 records checkpoints and terminal state. | Duplicate submission returns the existing identity; lease loss or restart resumes only from a durable checkpoint permitted by task semantics. | Unsafe or ambiguous recovery stops and requires a new attempt; abandoned work has a truthful terminal state. | Foreground and background work expose compatible lifecycle and evidence semantics. |

### Mermaid Control Flow [Conditionally Required — Control Flow Design is triggered]

```mermaid
flowchart TB
  subgraph CF1 ["CF-1 Foreground turn lifecycle"]
    CF1Start["Start or continue turn"] --> CF1Auth{"Identity and thread access valid?"}
    CF1Auth -->|"No"| CF1Reject["Reject before model or tool use"]
    CF1Auth -->|"Yes"| CF1Append["C-1 normalizes; C-2 appends started turn and input"]
    CF1Append --> CF1InputStored{"Started turn and input durable?"}
    CF1InputStored -->|"No"| CF1NoTurn["Reject with durability-unavailable; no turn accepted"]
    CF1InputStored -->|"Yes"| CF1Lease["C-2 acquires and renews fenced foreground lease through C-6"]
    CF1Lease --> CF1Resolve["Resolve context, capabilities, policy, and budgets"]
    CF1Lease -.->|"Heartbeat absent beyond liveness window"| CF1Orphan["Fence expired owner generation"]
    CF1Orphan --> CF1OrphanCancel["C-2 reconcilers race through one conditional idempotency key"] --> CF1Persist
    CF1Resolve --> CF1Provider["Invoke C-3"]
    CF1Provider --> CF1Outcome{"Next provider or control outcome"}
    CF1Outcome -->|"Stream item"| CF1Item["Append item through C-6"]
    CF1Item --> CF1ItemStored{"Item durable?"}
    CF1ItemStored -->|"Yes"| CF1Publish["C-1 publishes durable item"] --> CF1Provider
    CF1ItemStored -->|"No"| CF1StoreFail["Stop generation; emit out-of-band durability-unavailable"]
    CF1Outcome -->|"Retryable and budget remains"| CF1Provider
    CF1Outcome -->|"Authenticated client interrupt"| CF1Interrupt["Prepare interrupted terminal"] --> CF1Persist
    CF1Outcome -->|"Platform, policy, or dependency stop"| CF1Cancel["Prepare cancelled terminal"] --> CF1Persist
    CF1Outcome -->|"Terminal provider failure"| CF1Fail["Prepare failed terminal"] --> CF1Persist
    CF1Outcome -->|"Success"| CF1Persist["C-6 appends terminal outcome"]
    CF1Persist --> CF1Stored{"Append durable?"}
    CF1Stored -->|"No"| CF1StoreFail
    CF1Stored -->|"Yes"| CF1Done["Emit ordered durable terminal lifecycle"]
    CF1StoreFail --> CF1Recovery["Keep turn nonterminal; when C-6 returns, prepare failed terminal"] --> CF1Persist
  end

  subgraph CF2 ["CF-2 Capability policy, approval, and execution"]
    CF2Start["Model proposes capability action"] --> CF2Descriptor{"C-4 descriptor active and compatible?"}
    CF2Descriptor -->|"No or drifted"| CF2Refresh["Refresh and resolve exact descriptor"]
    CF2Refresh --> CF2Fresh{"Fresh compatible descriptor available?"}
    CF2Fresh -->|"No"| CF2NoExec["Typed non-execution result"]
    CF2Fresh -->|"Yes"| CF2Policy["C-5 validates lease generation, input, and effect"]
    CF2Descriptor -->|"Yes"| CF2Policy
    CF2Policy --> CF2Lease{"Foreground lease current or not applicable?"}
    CF2Lease -->|"No"| CF2NoExec
    CF2Lease -->|"Yes"| CF2Decision{"Policy decision"}
    CF2Decision -->|"Deny or require narrower input"| CF2NoExec["Typed non-execution result"]
    CF2Decision -->|"Approval required"| CF2Present["C-1/C-7 presents canonical exact D-6 request"]
    CF2Present --> CF2Approval{"Accept, decline, cancel, or expire?"}
    CF2Approval -->|"Decline / cancel / expire"| CF2NoExec
    CF2Approval -->|"Accept one exact D-7 attempt"| CF2Execute["Dispatch to isolated executor"]
    CF2Decision -->|"Allow"| CF2Execute
    CF2Execute --> CF2ExecOutcome{"Execution outcome"}
    CF2ExecOutcome -->|"Pre-effect retryable failure and metadata permits"| CF2Reevaluate["Create new attempt candidate; reevaluate policy and approval"] --> CF2Policy
    CF2ExecOutcome -->|"Timeout or cancellation"| CF2Stop["Terminate attempt with typed outcome"]
    CF2ExecOutcome -->|"Owner generation fenced"| CF2Fenced["Reject result commit; record cancelled or failed from observed effect state"]
    CF2ExecOutcome -->|"Failure or effect may have occurred"| CF2Failure["Record typed failed attempt; do not auto-retry"]
    CF2ExecOutcome -->|"Success"| CF2Result["Return D-6 projection and untrusted result to C-2"]
    CF2NoExec --> CF2Audit["Link descriptor, policy, applicable D-6/D-7, result, and projections"]
    CF2Stop --> CF2Audit
    CF2Fenced --> CF2Audit
    CF2Failure --> CF2Audit
    CF2Result --> CF2Audit
  end

  subgraph CF3 ["CF-3 Resume or fork"]
    CF3Start["Resume or fork request"] --> CF3Access{"Caller has access?"}
    CF3Access -->|"No"| CF3Reject["Reject without creating work"]
    CF3Access -->|"Yes"| CF3Load["C-6 loads canonical history and lineage"]
    CF3Load --> CF3History{"Canonical history complete and valid?"}
    CF3History -->|"No"| CF3Fail["Fail visibly; do not truncate history"]
    CF3History -->|"Yes"| CF3Cache{"Optional cache available?"}
    CF3Cache -->|"No"| CF3Rebuild["Rebuild cache from canonical history"] --> CF3Version
    CF3Cache -->|"Yes"| CF3Version{"Historical item version compatible?"}
    CF3Version -->|"No, adapter exists"| CF3Translate["Apply versioned translation"] --> CF3Context
    CF3Version -->|"No adapter"| CF3Fail
    CF3Version -->|"Yes"| CF3Context["Reconstruct context under current budget"]
    CF3Context --> CF3Operation{"Resume or fork?"}
    CF3Operation -->|"Resume"| CF3Resume["Start a new turn on the same thread; prior terminal turn stays terminal"]
    CF3Operation -->|"Fork"| CF3Fork["Create child thread, then start a new turn with stable lineage"]
  end

  subgraph CF4 ["CF-4 Extension inventory refresh"]
    CF4Start["Configured source changes or becomes unavailable"] --> CF4Reachable{"Source reachable?"}
    CF4Reachable -->|"No, stale policy permits age and scope"| CF4Prior["Keep prior valid snapshot and report stale status"]
    CF4Reachable -->|"No stale permission"| CF4Fail["Fail closed with capability-unavailable diagnostics"]
    CF4Reachable -->|"Yes"| CF4Discover["Discover, parse, validate, and record provenance"]
    CF4Discover --> CF4Valid{"Entry valid?"}
    CF4Valid -->|"No"| CF4Exclude["Exclude entry and emit diagnostics"] --> CF4Snapshot
    CF4Valid -->|"Yes"| CF4Snapshot["Build coherent candidate snapshot"]
    CF4Snapshot --> CF4Publish{"Atomic publish succeeds?"}
    CF4Publish -->|"Yes"| CF4Done["New turns use new snapshot; in-flight turns retain resolved snapshot"]
    CF4Publish -->|"No, stale policy permits age and scope"| CF4Prior
    CF4Publish -->|"No stale permission"| CF4Fail
  end

  subgraph CF5 ["CF-5 Background work"]
    CF5Start["Submit background work"] --> CF5Validate{"Identity, idempotency, and policy valid?"}
    CF5Validate -->|"No"| CF5Reject["Reject without scheduling"]
    CF5Validate -->|"Duplicate"| CF5Existing["Return existing work identity"]
    CF5Validate -->|"Yes"| CF5Intent["Persist work intent and schedule with Multitask"]
    CF5Intent --> CF5Worker["Worker runs the same core lifecycle"]
    CF5Worker --> CF5Checkpoint["C-6 records checkpoint and lease progress"]
    CF5Checkpoint --> CF5Outcome{"Terminal, lease loss, or restart?"}
    CF5Outcome -->|"Terminal"| CF5Done["Record truthful compatible terminal state"]
    CF5Outcome -->|"Recoverable from permitted checkpoint"| CF5Worker
    CF5Outcome -->|"Unsafe or ambiguous recovery"| CF5Stop["Stop; record abandonment and require a new attempt"]
  end
```

## Interaction Flow Design [Conditionally Required — a human or external system interacts with the solution]

| ID | Actor and entry state | Actions | System feedback and transitions | Exit state | Figma reference |
| --- | --- | --- | --- | --- | --- |
| IX-1 | API client with authenticated principal and an existing or new thread | Start, steer, interrupt, resume, fork, or read a thread. | C-1 obtains trust context from C-7. Every lifecycle item is published only after C-6 confirms durability. Resume loads canonical history and creates a new turn on the same thread; the source turn remains terminal. `Interrupted` reports an authenticated client stop; `cancelled` reports a platform, policy, dependency, or reconciled foreground-owner stop. | A durable terminal turn, a new turn created by Resume, or a rejected request with no side effect. Initial durability failure creates no turn; a later outage exposes only the durable prefix plus `durability-unavailable`; an orphan becomes durably `cancelled` after the liveness window. | N/A — service/protocol interaction only; no UI is designed here. |
| IX-2 | Human approver using the authenticated approval protocol exposed by C-1 and validated through C-7 | Inspect the canonical D-6 action, target, parameters, effect, scope, rationale, and risk; accept, decline, or cancel that exact request. | C-5 remains the D-6 authority. C-1 carries the request and decision but does not own approval state; C-2/C-6 append user-visible D-3 status projections referencing D-6. Acceptance binds exactly one D-7 attempt, and the execution result is reported separately. | Declined/cancelled/expired with no execution, or accepted with one linked terminal D-7 attempt. Any retry is a newly evaluated attempt and approval when required. | N/A — approval protocol semantics only; presentation design requires future Figma context. |
| IX-3 | MCP, tool, model, memory, auth, or multitask system | Initialize or negotiate, exchange versioned requests/events, report capabilities, and return results. | Compatibility, deadline, correlation, retryability, and terminal status are explicit. | Success, compatible degradation, or typed failure without authority escalation. | N/A — external-system interaction. |

### Mermaid Interaction Flow [Conditionally Required — Interaction Flow Design is triggered]

```mermaid
sequenceDiagram
  participant Client as API client
  participant Approver as Human approver
  participant C1 as C-1 Presentation boundary
  participant Identity as C-7 Identity adapter
  participant Core as C-2 Agent core
  participant Store as C-6 Thread store
  participant Policy as C-5 Policy and execution
  participant External as IX-3 External system

  Note over Client,Store: IX-1 Client lifecycle interaction
  Client->>C1: Start, steer, resume, fork, read, or interrupt
  C1->>Identity: Validate signed claims and requested ownership
  Identity-->>C1: Immutable trust context or typed rejection
  alt Identity or request rejected
    C1-->>Client: Typed rejection with no side effect
  else Request accepted
    C1->>Core: Owned thread or turn operation
    Core->>Store: Append or load ordered state and lineage
    Store-->>Core: Durable state, checkpoint, or typed failure
    alt Storage unavailable
      alt No durable started turn exists
        Core-->>C1: Reject operation; no turn was accepted
        C1-->>Client: Durability-unavailable rejection with no side effect
      else Durable started turn exists
        Core-->>C1: Stop work; no unpersisted item is publishable
        C1-->>Client: Out-of-band durability-unavailable notification
        opt C-6 recovers
          Core->>Store: Append failed terminal for recovery-pending turn
          Store-->>Core: Durable failed terminal acknowledgement
        end
      end
    else Active work
      opt Operation is Resume
        Core->>Store: Append a new turn on the same thread from canonical history
        Store-->>Core: New turn identity and durable lineage
        Note over Core,Store: The source terminal turn remains unchanged
      end
      Core->>Store: Append next lifecycle item
      Store-->>Core: Durable item acknowledgement
      Core-->>C1: Durable progress or approval-status projection
      C1-->>Client: Ordered replayable feedback
      alt Authenticated client interrupts
        Client->>C1: Interrupt exact active turn
        C1->>Core: Interrupt active work
        Core->>Store: Persist interrupted terminal state
        Store-->>Core: Durable terminal acknowledgement
        C1-->>Client: Explicit interrupted feedback
      else Platform, policy, dependency, or orphan reconciler cancels
        Core->>Store: Persist cancelled terminal state
        Store-->>Core: Durable terminal acknowledgement
        C1-->>Client: Explicit cancelled feedback
      else Work reaches normal terminal
        Core->>Store: Persist completed or failed terminal outcome
        Store-->>Core: Durable terminal acknowledgement
        Core-->>C1: Completed or failed terminal event
        C1-->>Client: Durable terminal feedback
      end
    end
  end

  Note over Approver,Policy: IX-2 Human approval interaction
  Core->>Policy: Propose exact privileged action and scope
  alt Approval required
    Policy-->>Core: Canonical pending D-6 identity and projection
    Core->>Store: Append D-3 projection referencing D-6
    Core-->>C1: Present canonical exact D-6 request
    C1-->>Approver: Show action, target, parameters, effect, scope, rationale, and risk
    alt Decision arrives before expiry
      Approver->>C1: Accept, decline, or cancel exact D-6 identity
      C1->>Identity: Validate approver identity and scope
      Identity-->>C1: Immutable approver trust context or rejection
      alt Approver identity rejected
        C1-->>Approver: Typed rejection; request remains pending until expiry
      else Approver identity valid
        C1->>Policy: Validated decision for exact D-6 identity
        alt Approver accepts exact scope
          Policy->>External: Execute one bounded D-7 attempt
          External-->>Policy: Typed terminal result
          Policy-->>Core: Canonical D-6/D-7 terminal status and projection
          Core->>Store: Append D-3 projection referencing D-6/D-7
          Core-->>C1: Separate execution result
          C1-->>Approver: Report linked terminal attempt
        else Approver declines or cancels
          Policy-->>Core: Canonical non-execution status and projection
          Core->>Store: Append D-3 projection referencing D-6
          Core-->>C1: Confirm no execution
          C1-->>Approver: Report declined or cancelled
        end
      end
    else Request expires before a valid decision
      Policy-->>Core: Canonical expired status and projection
      Core->>Store: Append D-3 projection referencing D-6
      Core-->>C1: Expiry notification
      C1-->>Approver: Report expiry
    end
  else Policy allows without approval
    Policy->>External: Execute within resolved profile
    External-->>Policy: Typed terminal result
    Policy-->>Core: Linked terminal attempt
  else Policy denies
    Policy-->>Core: Typed non-execution result
  end

  Note over Core,External: IX-3 External-system interaction
  Core->>External: Initialize or negotiate version and capabilities
  External-->>Core: Compatible capabilities or typed incompatibility
  alt Compatible
    Core->>External: Versioned request with deadline and correlation
    alt Successful terminal response
      External-->>Core: Correlated events and success
    else Retryable pre-effect failure within budget
      External-->>Core: Typed retryable failure
      alt Privileged effect
        Core->>Policy: Create new D-7 candidate and reevaluate policy
        Policy-->>Core: Fresh approval required when applicable
      else Non-effect idempotent dependency call
        Core->>Core: Apply declared retry and budget metadata
      end
      Core->>External: Preserve logical correlation and use a new attempt identity
      External-->>Core: Terminal response
    else Cancellation or deadline
      Core->>External: Cancel exact operation
      External-->>Core: Cancelled or timed-out terminal status
    else Non-retryable failure
      External-->>Core: Typed terminal failure without added authority
    end
  else Incompatible
    Core-->>C1: Compatible degradation when defined, otherwise typed failure
  end
```

## Cross-Cutting Design [Required]

| Quality attribute | Solution-level design | Architecture-level validation |
| --- | --- | --- |
| Security and least privilege | Immutable identity context, named permission profiles, default deny, bounded approvals, isolated execution, network/filesystem/process controls, and untrusted-output treatment. | A review matrix demonstrates that every privileged effect has one policy owner, one enforcement boundary, a deny path, and an audit result; no core path executes host effects directly. |
| Privacy and secret safety | Credentials stay in adapter configuration; prompts, tool arguments/results, paths, and logs are minimized and redacted according to classification. Thread deletion removes user-content history and approval projections under owner policy, while canonical D-6/D-7 security evidence follows a separately defined retention/deletion schedule with minimized or pseudonymized linkage. | Deterministic inspection shows no secret field in thread history or diagnostics contracts, no audit payload survives beyond its approved security/privacy retention, and all optional content diagnostics require explicit safe-environment gating. |
| Reliability | Explicit lifecycle states, cancellation, timeout, retry and token budgets, idempotency, append-before-publish ordering, fenced foreground liveness leases, checkpoint ownership, and truthful partial/terminal outcomes. | Each externally visible item is durable before publication. Cancellation, timeout, dependency failure, storage failure, approval decline, duplicate input, and foreground owner loss each have an exact state; lease expiry produces exactly one `cancelled` terminal and rejects stale-owner writes. |
| Observability | Correlation across thread, turn, item, provider call, capability descriptor, approval, and execution attempt. | Traceability review can follow one request from ingress to its durable terminal state without relying on sensitive content. |
| Contract evolution | New northbound REST/SSE and owned store contracts are versioned and provider neutral; predecessor contracts are research evidence only. | Contract tests verify the new C-1 and C-6 contracts directly. Later incompatible changes require separate accepted decisions; no legacy parity or route-back gate applies. |
| Maintainability | One coherent owner for orchestration, presentation, provider translation, extension discovery, policy/execution, identity, and storage. | Dependency review shows inward dependencies toward owned contracts, no cycles, and no external provider/storage/transport types in the core. |
| Scalability | Stateless presentation/core instances where practical; durable shared state behind adapters; foreground turns use fenced liveness leases, and background execution uses leases and checkpoints. | Architecture review demonstrates horizontal instances do not require process-local truth; any healthy C-2 instance can reconcile an orphan without accepting stale-owner writes or duplicating non-idempotent effects. |
| Supply-chain and provenance | External reference concepts are pinned to immutable revisions; extensions and capability descriptors carry source and version provenance. | Evidence links resolve to immutable commits, and a catalog snapshot can identify the source/version of every active extension. |

## Assumptions And Open Questions [Conditionally Required — assumptions or material questions exist]

| ID | Assumption or question | Owner | Status | Resolution and evidence |
| --- | --- | --- | --- | --- |
| Q-1 | What is the AI-service research baseline when this repository has no service code? | @linhai | Resolved | Use the predecessor `koduck-quant` `koduck-ai` tree at commit `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe` only for functional research. The current repository [README](../../README.md) identifies Koduck as a from-scratch rebuild with no service, and repository-owner direction on 2026-08-11 confirms the predecessor infrastructure is removed and is not an operating baseline. |
| Q-2 | Which Codex revision is the comparison baseline? | @linhai | Resolved | Use public `openai/codex` commit `3c60d4da648bfa98e3c51c5161ac2720519c733e`, observed from `refs/heads/main` on 2026-08-10. The 2026-08-10 ADD review by @linhai confirmed the immutable evidence baseline. |
| Q-3 | Does “align with Codex” mean fork it or reproduce all product behavior? | @linhai | Resolved | No. The Trello outcome asks for boundaries and a migration proposal, and this ADD selects conceptual alignment with owned Koduck contracts. Forking and parity are explicit non-goals subject to approval with this ADD. |
| Q-4 | Which document is authoritative when the English and Chinese files differ? | @linhai | Resolved | The indexed English file `docs/architecture/ADD-0001-ai-service-codex-alignment.md` is authoritative; the Chinese file is a non-authoritative synchronized translation. |
| Q-5 | What operating model applies when the predecessor infrastructure has been removed? | @linhai | Resolved | Repository-owner direction in the active Codex task on 2026-08-11 establishes a greenfield model: new implementation contracts are authoritative; the old baseline is functional research evidence only; no predecessor artifact, APISIX route, shared history, fallback, or route-back gate applies. |

No material question remains open for approval of this design. An approver may
return the document to `Draft` by identifying a material unresolved issue
instead of responding `Approve`.

## Risks And Trade-Offs [Required]

| ID | Risk or trade-off | Impact | Mitigation |
| --- | --- | --- | --- |
| RK-1 | Treating Codex structure as a copy target rather than evidence. | Koduck could inherit irrelevant local-product, auth, UI, and persistence constraints. | Use the adoption matrix; every implementation decision must cite a Koduck outcome and be approved in its own ADR. |
| RK-2 | Research evidence is mistaken for a compatibility obligation. | New contracts could inherit removed infrastructure and unverified wire behavior. | Label predecessor material as functional evidence only and test the new owned contract directly. |
| RK-3 | Splitting a monolith creates distributed coupling instead of boundaries. | More crates or services without ownership clarity can worsen change cost and latency. | Define ports by failure, trust, data, and lifecycle boundaries; do not split solely by file size. |
| RK-4 | Approval UI exists but enforcement remains in-process and bypassable. | Privileged effects may execute without the reviewed scope. | Bind decisions at C-5 and enforce the same policy below every execution path, including MCP and background workers. |
| RK-5 | Canonical history and semantic memory ownership overlap. | Duplicate truth, inconsistent replay, or cross-tenant leakage. | Keep Thread/Turn/Item in the AI-owned store; CAND-3 must define versioned projections and ownership for Memory/Multitask integration. |
| RK-6 | The first owned REST/SSE contract omits a required functional scenario. | A greenfield release could be internally consistent but incomplete. | Derive scenario coverage from predecessor research and current product requirements, while making the new versioned contract and deterministic tests authoritative. |
| RK-7 | Extension descriptions or tool results manipulate authority. | Prompt injection could cause unauthorized behavior or data access. | Treat all extension/model/tool content as untrusted; authorization comes only from identity, policy, and explicit approval. |
| RK-8 | Multi-agent expansion multiplies unfinished lifecycle and permission risks. | Concurrency, lineage, budget, and approval semantics become ambiguous. | Defer proactive multi-agent execution until CAND-1 through CAND-4 are complete and verified. |
| RK-9 | One-attempt approval increases prompts and latency compared with reusable session/turn grants. | High-frequency privileged workflows may be slower or encourage unsafe pressure to bypass approval. | Keep the CAND-2 baseline exact and auditable; any reusable grant requires measured need, a bounded revocation/scope model, and a separate Accepted ADR. |
| RK-10 | Append-before-publish couples stream latency and availability to C-6. | First-item latency may increase, and a store outage stops otherwise usable model output. | CAND-1 must set and measure bounded append/backpressure thresholds, preserve the durable-prefix invariant, and fail closed; weakening durability requires a separate architecture decision. |
| RK-11 | Foreground liveness windows can falsely classify a paused or partitioned owner as dead, or delay orphan closure when too long. | A live computation may be cancelled, while an overly conservative window leaves users waiting. | C-6 generation fencing makes the decision safe; CAND-1 must bound heartbeat, clock-skew, pause, and partition cases, and cancellation never transfers the same Turn to another owner. |

## ADR Task Candidates [Required]

Allowed task-candidate statuses: `Ready`, `Selected`, `Complete`, or `Deferred`.

| ID | Complete outcome | Scope boundary | Dependencies | Acceptance context | Recommended ADR type | Status | Status reason or evidence | ADR path |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CAND-1 | A provider-neutral thread/turn/item orchestration kernel can execute one authenticated, tool-free turn through a new versioned REST/SSE boundary with durable ordered history and explicit completion, failure, interruption, durability-outage, and foreground-owner-loss outcomes. | Includes the owned lifecycle model, core ports, new REST/SSE v1 contract, one provider path, and an AI-owned durable C-6 adapter sufficient for append-before-publish, replay, and fenced foreground liveness leases; excludes semantic Memory integration, background Multitask integration, forks/checkpoints, privileged tools, extensions, deployment, and any legacy compatibility or fallback path. | This ADD must be `Current`; the new REST/SSE v1 and TurnHistory contracts, trust-context handoff, and AI-owned durable-store boundary must be complete and deterministic before ADR acceptance. | Binary contract checks for one non-tool turn; deterministic state, Resume-as-new-Turn, append-before-publish, bounded append/backpressure and liveness windows, replay, provider/store failure, process crash, lease expiry, stale-owner fencing, concurrent-reconciler idempotency, and exactly-one orphan `cancelled` terminal. | Full | Complete | Completed by the Accepted, Complete project Full ADR at `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; implementation source is commit `08cc1b3`, review corrections are commits `56073a0`, `df49b69`, `11b5ea2`, `fe3beb9`, `a7258bc`, `a7b6faa`, and `31ef43f`, and all 14 ADR checks pass | `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` |
| CAND-2 | Every tool and MCP invocation passes through one default-deny policy and isolated D-7 execution boundary; any required approval uses one canonical exact-action D-6 record, with cancellation, timeout, output-cap, lease fencing, and an auditable terminal result. | Includes C-1/C-7 approval transport, C-5 authority, C-6 foreground-lease validation, D-3 status projections, and new Tool/MCP adapters; excludes reusable session/turn grants, UI design, and expansion of allowed privileged capabilities. | CAND-1 complete; authenticated approval protocol and intended tool-effect inventory available. | Checks cover allow without approval, deny, invalid approver identity, decline, cancel, expiry, scope/attempt/lease drift, stale-owner dispatch and result rejection, pre-effect retry reapproval, timeout, cancellation, and untrusted output; recovery disables or reverts the unpromoted dispatcher and leaves tools unavailable rather than invoking a legacy path. | Full | Ready | N/A — Ready; dependency prevents selection before CAND-1 completes | None |
| CAND-3 | The AI-owned store port expands to canonical lineage, checkpoints, and idempotency while Memory and Multitask integrate through separate versioned semantic-memory and background-work contracts without process-local truth. | Extends CAND-1 history/liveness ownership with fork, checkpoint, background-resume, projection, and integration semantics; excludes unrelated memory ranking and deployment. Memory and Multitask do not replace AI ownership of canonical Thread/Turn/Item. | CAND-1 complete; Memory and Multitask contract owners participate. | Replay/order/append-only-correction/lease-fencing/tenant-isolation and duplicate-submission checks; schema evolution preserves CAND-1 history and terminal semantics, while rollback uses the last verified new schema/artifact and discards only reconstructable projections. | Full | Ready | N/A — Ready; dependency prevents selection before CAND-1 completes | None |
| CAND-4 | Repository instructions, agent profiles, skills, plugins, and MCP descriptors load through one provenance-bearing extension boundary and cannot widen execution permissions. | Includes discovery, validation, precedence, snapshots, diagnostics, and current extension adapters; excludes marketplace UI, remote installation, and new privileged tools. | CAND-1 and CAND-2 complete; CAND-3 storage snapshot semantics available where persistence is required. | Deterministic precedence, invalid-extension, source-loss, stale-policy, isolation, snapshot-consistency, and permission-non-escalation checks; rollback disables the new registry and retains static known-safe configuration. | Full | Ready | N/A — Ready; dependencies prevent early selection | None |
| CAND-5 | Foreground and background model turns use the new core across supported providers and satisfy the owned REST/SSE, lifecycle, recovery, and service-readiness contracts for first production promotion. | Includes provider adapters, background lifecycle integration, consumer readiness, SLO evidence, and first-release readiness; excludes new product features and UI redesign. | CAND-1 through CAND-4 complete and verified; intended consumer inventory complete. | Provider/stream/background contract, recovery, SLO, error-budget, and promotion-stop checks; before first promotion failure quarantines the candidate, and later rollback may target only a last verified new artifact under an OCR. | Full | Ready | N/A — Ready; final readiness candidate | None |
| CAND-6 | Multi-agent execution has an approved lifecycle, lineage, budget, permission, approval, cancellation, and storage model or is explicitly rejected after evidence review. | Includes an architecture decision and bounded pilot outcome; excludes production rollout and UI design. | CAND-1 through CAND-5 complete and verified; single-agent metrics and incident evidence available. | Decision is supported by measured need and a deterministic safety/ownership review; rejection or deferral leaves no dormant production path. | Full | Deferred | Deferred until the single-agent target boundary is complete and evidence demonstrates a need | None |

## Traceability [Required]

| Requirement | Capabilities | Data entities | Components | Control / interaction flows | ADR task candidates |
| --- | --- | --- | --- | --- | --- |
| R-1 | F-1, F-2, F-3, F-4, F-5, F-6, F-7 | D-1 through D-8 | C-1 through C-8 | CF-1 through CF-5; IX-1 through IX-3 | CAND-1 through CAND-6 |
| R-2 | F-8 | N/A — translation creates no runtime data entity | N/A — translation adds no runtime component | N/A — documentation review only | N/A — fulfilled by the synchronized translation, not an implementation ADR |

## Supporting Material [Optional]

### Evidence Baselines

| Evidence ID | Immutable or repository source | Material conclusion |
| --- | --- | --- |
| E-1 | Current repository [README](../../README.md) | Koduck is a from-scratch rebuild of `koduck-quant`; no service has yet been added here. |
| E-2 | [`koduck-ai` design at `c414ddcc`](https://github.com/hailingu/koduck-quant/blob/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai/docs/design/ai-decoupled-architecture.md) | The predecessor intends `koduck-ai` to be an AI gateway/orchestrator and assigns memory, tools, auth, and gateway governance to surrounding services. |
| E-3 | [`koduck-ai/src/lib.rs` at `c414ddcc`](https://github.com/hailingu/koduck-quant/blob/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai/src/lib.rs) and [`app/mod.rs`](https://github.com/hailingu/koduck-quant/blob/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai/src/app/mod.rs) | One crate contains API, app lifecycle, auth, background work, clients, configuration, context, LLM, MCP, orchestration, registry, reliability, session, skill, storage, streaming, and tasks, and exposes broad REST/SSE behavior. |
| E-4 | [`native_tool_loop.rs` at `c414ddcc`](https://github.com/hailingu/koduck-quant/blob/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai/src/api/llm_flow/native_tool_loop.rs) | Tool orchestration is concentrated in one 2,073-line source unit, indicating a high-coupling review area for later ADR design. |
| E-5 | [`mcp/mod.rs` at `c414ddcc`](https://github.com/hailingu/koduck-quant/blob/c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe/koduck-ai/src/mcp/mod.rs) | Koduck already supports a deliberately small MCP client surface and adapts discovered tools into its native tool pipeline. |
| E-6 | [`app-server` README at `3c60d4da`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/app-server/README.md) | Codex separates a bidirectional application protocol and models threads, turns, typed events, approvals, skills, apps, auth, and command execution at that boundary. |
| E-7 | [`thread-store` README at `3c60d4da`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/thread-store/README.md) | Codex defines a replaceable `ThreadStore`, one metadata-write API, a `LiveThread` abstraction, and local/in-memory implementations. |
| E-8 | [`core` README at `3c60d4da`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/core/README.md), [`sandboxing`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/core/src/sandboxing/mod.rs), and [`approvals`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/core/src/tools/approvals.rs) | Codex treats filesystem/network sandbox selection and approval as explicit execution concerns rather than model-granted authority. |
| E-9 | [`codex_mcp_interface.md` at `3c60d4da`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/docs/codex_mcp_interface.md) | The public control surface uses thread/turn operations, typed notifications, and server-to-client approval requests; the MCP control interface is explicitly experimental. |
| E-10 | [`agents_md.rs`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/core/src/agents_md.rs), [`skills/loading.rs`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/skills/src/loading.rs), and [`plugins/mod.rs`](https://github.com/openai/codex/blob/3c60d4da648bfa98e3c51c5161ac2720519c733e/codex-rs/core/src/plugins/mod.rs) | Repository instructions, skills, and plugins are separately loaded capabilities with explicit roots and services. |

### Current-To-Target Gap Matrix

| Area | Current predecessor baseline | Codex reference signal | Koduck target boundary | Gap disposition |
| --- | --- | --- | --- | --- |
| Lifecycle model | Session/chat/task concepts are spread across REST handlers, native loops, memory clients, task registries, and workers. | Explicit thread, turn, typed item, lifecycle events, resume/fork/interrupt. | One owned thread/turn/item domain used by foreground and background flows. | Adjusted adoption; translate existing session/task semantics instead of copying Codex wire types. |
| Application protocol | The predecessor exposes REST/SSE routes, but they are research evidence rather than a live contract. | Typed bidirectional app-server protocol with generated schemas. | Define an owned versioned REST/SSE v1 boundary and add a provider-neutral typed application protocol when a consumer requires it. | Adjusted adoption without legacy wire parity. |
| Core ownership | One crate combines transport, orchestration, provider, tools, MCP, background, storage, and policy concerns. | `codex-core` is consumed by multiple UIs; protocol, storage, execution, sandbox, skills, and app server are distinct packages. | Provider-neutral core with ports; split only at ownership, failure, trust, or lifecycle boundaries. | Adopt boundary principle, not exact crate list. |
| Tool orchestration | Native model tool use and catalog dispatch exist; policy is split across foreground, background allowlists, tool service, and selected approval logic. | Central tool routing plus explicit approval and sandbox policy. | One policy/approval/execution boundary below every tool path. | Adjusted adoption; keep Tool service and MCP adapters. |
| Sandboxing | Service isolation and allowlists exist, but no universal turn-scoped filesystem/network/process sandbox contract is evident across all tool paths. | Platform-specific sandboxing and named permission profiles. | Execution effects run in an isolated worker or platform sandbox with explicit profiles and deny-by-default enforcement. | Adopt security model; platform implementation remains an ADR choice. |
| Storage | The predecessor used Memory/Multitask plus process-local registries and checkpoints; that infrastructure is no longer an operating baseline. | Replaceable `ThreadStore`; local rollout JSONL and SQLite-backed metadata. | Owned store port backed by an AI-owned shared PostgreSQL datastore; Memory and Multitask later consume separate semantic-memory/background contracts. | Adopt the store abstraction and shared durability, not predecessor ownership or process-local truth. |
| Provider layer | The predecessor demonstrates multi-provider adapters, routing, normalized types, streaming, retry, and fallback, but is research evidence only. | Core client and model-provider packages focus strongly on OpenAI/Codex product needs. | Start with one explicitly selected provider and no automatic fallback; preserve the adapter boundary so later providers require their own accepted decision. | Adopt the boundary, not predecessor fallback behavior. |
| MCP | Custom minimal stdio/HTTP MCP client adapts tools into the native loop. | MCP clients, resources, approvals, control server, and application integration are separate concerns; some surfaces are experimental. | Standards-compliant MCP adapter with version/capability negotiation, provenance, elicitation/approval routing, and untrusted output handling. | Adjusted adoption; avoid binding canonical Koduck APIs to experimental Codex control RPCs. |
| Instructions/skills/plugins | Agent profile, skill, and MCP modules exist with uneven runtime activation and ownership. | Repository instructions, skill-root loading, plugins, and injections have distinct loaders/services. | One extension registry with deterministic precedence, snapshots, validation, provenance, and permission non-escalation. | Adopt with adjustment for tenant/thread isolation. |
| Auth and identity | APISIX/JWT/JWKS and tenant/user claims anchor access. | ChatGPT account login and product-specific auth helpers. | Retain APISIX/Auth/JWKS identity and pass immutable trust context into the core. | Do not adopt Codex auth model. |
| Observability | Structured tracing, reliability metrics, and guarded prompt diagnostics exist, but evidence spans many paths. | Typed lifecycle and approval/execution events provide explicit UI and audit signals. | Correlated typed events across ingress, turn, provider, tool, approval, storage, and recovery. | Adopt event discipline; preserve privacy constraints. |
| Multi-agent | Background task and plan lineage exist; proactive subagent semantics are not a proven product requirement. | Agent spawning, messaging, wait, lineage, and collaboration modes exist. | Defer until the single-agent execution and store boundaries are verified and measured demand exists. | Do not adopt now. |

### Adoption Decisions

| Decision | Codex concept | Koduck disposition | Reason |
| --- | --- | --- | --- |
| AD-1 | Thread/turn/item lifecycle with typed events | Adopt after adjustment | It gives one replayable lifecycle, but Resume creates a new turn, terminal turns never reactivate, corrections are append-only items, and existing Koduck session/task identities map into owned types. |
| AD-2 | Provider-independent core consumed by multiple presentation surfaces | Adopt | It directly addresses transport/provider/orchestration coupling and supports REST/SSE plus future clients. |
| AD-3 | Replaceable thread store | Adopt after adjustment | CAND-1 establishes the consumer-owned port and AI-owned shared PostgreSQL adapter as canonical for Thread/Turn/Item; CAND-3 expands lineage/checkpoint/idempotency and integrates Memory/Multitask without transferring canonical ownership. |
| AD-4 | Named permission profiles, bounded approvals, and platform/worker isolation | Adopt after adjustment | Model or extension output never grants authority. C-5 owns canonical D-6 records, C-1/C-7 carry authenticated decisions, D-3 is projection only, and the initial safety baseline authorizes one exact D-7 attempt rather than a reusable session/turn grant. |
| AD-5 | Separate application protocol and generated schemas | Adopt after adjustment | Versioned types improve compatibility, but Codex app-server methods and experimental MCP control RPCs are not Koduck contracts. |
| AD-6 | Repository instructions, skills, plugins, and MCP as separately loaded extension capabilities | Adopt after adjustment | Koduck needs tenant/thread isolation, provenance, and non-escalating permission semantics across these sources. |
| AD-7 | Codex local rollout files and SQLite as canonical state | Do not adopt | Koduck requires shared multi-instance state in an AI-owned durable datastore; process-local rollout files remain non-canonical. |
| AD-8 | ChatGPT account/auth/rate-limit/model-catalog behavior | Do not adopt | Koduck's APISIX/JWT/JWKS and multi-provider model are authoritative product boundaries. |
| AD-9 | Codex CLI/TUI/desktop UI and filesystem operations API | Do not adopt as product scope | UI requires its own Figma context, and broad filesystem APIs are not required for the first service migration. |
| AD-10 | Proactive multi-agent runtime | Do not adopt yet | It compounds lifecycle, permission, cost, cancellation, and storage risks before foundational boundaries are verified. |
| AD-11 | Direct code fork or crate-for-crate rewrite | Do not adopt | Product requirements, external contracts, storage, auth, and deployment differ; conceptual alignment offers lower coupling and clearer ownership. |

### External Contract And Security Boundary Inventory

| Boundary | Existing contract to preserve or assess | Target owner | Security and compatibility rule |
| --- | --- | --- | --- |
| Client or approver to AI | New versioned REST/SSE chat and approval protocol | C-1 with C-7 identity validation | The owned contract is authoritative; C-1 delegates signed-claim validation to C-7 before state/model/tool/approval work; bounded bodies, ordered durable stream terminals, and exact D-6 identities are mandatory. Predecessor routes supply functional scenarios only. |
| Gateway/Auth to AI | APISIX routing and JWT/JWKS-derived tenant/user identity | C-7 | Signed claims are authoritative; forwarded headers cannot invent identity; JWKS failures follow explicit fail-closed/stale-key policy. |
| AI to Memory | Future versioned semantic-memory projection and retrieval contract | Dedicated adapter outside canonical C-6 ownership | Tenant/user/thread ownership, deadlines, idempotency, version negotiation, and explicit separation from canonical turn history are mandatory. |
| AI to Multitask | Future background submission, lease, checkpoint, retry, and terminal-state contract | Background-work adapter coordinated with C-2/C-6 | Duplicate submissions map to one logical job; lease loss cannot duplicate non-idempotent effects; credentials never enter history; Multitask does not own foreground canonical turns. |
| AI to Tool | Capability discovery, schema validation, and execution | C-4/C-5 adapter | Descriptor provenance, exact version, default deny, idempotency, timeout, output cap, and audit apply. |
| AI to MCP | JSON-RPC initialization, discovery, invocation, resources, and elicitation as supported | C-4/C-5 adapter | Server content is untrusted; transport does not grant authority; approvals and filesystem/network/process access remain locally enforced. |
| AI to model provider | Provider-native HTTP/streaming | C-3 | Secrets stay at adapter boundary; provider/model selection is explicit; CAND-1 has no fallback; redaction, token/time/retry limits, and normalized terminals apply. |
| Core to executor | Owned action, profile, approval decision, and execution event contract | C-5 | Strongest trust boundary: no bypass, exact scope binding, isolation, cancellation, timeout, output limits, and audit evidence. |
| Extensions to core | Instruction, profile, skill, plugin, and tool descriptors | C-4 | Deterministic precedence, provenance, schema validation, tenant/thread isolation, and zero permission escalation by content. |

### Ordered Greenfield Delivery Slices, Validation, And Recovery Boundaries

The candidate order is CAND-1, CAND-2, CAND-3, CAND-4, and CAND-5.
CAND-6 remains deferred. CAND-1 establishes the new C-1 contract and AI-owned
C-6 durable boundary; CAND-3 expands storage and integrates Memory/Multitask
through separate responsibilities. No slice assumes a predecessor deployment,
legacy route, shared-history subset, or fallback. Each selected candidate
requires one reciprocal Full ADR with no more than three implementation
subtasks and deterministic checks.

| Slice | Minimum architecture outcome | Architecture-level validation | Recovery boundary |
| --- | --- | --- | --- |
| 1 / CAND-1 | One tool-free authenticated turn crosses the new REST/SSE v1 boundary → core → provider → AI-owned C-6 adapter and reaches one explicit durable terminal state. | Owned-contract mapping, Resume-as-new-Turn, append-before-publish latency/backpressure, foreground liveness window, process crash, lease expiry, stale-owner fencing, exactly-one orphan cancellation, ordered replay, and provider/store failure all have binary states. | Before any promotion, quarantine or revert a failing candidate and retain its deterministic evidence; there is no predecessor route-back. After a verified new release exists, an OCR may select only a verified new artifact. |
| 2 / CAND-2 | All tool/MCP effects cross one C-5 authority and isolated one-attempt D-7 boundary; required approvals use exact D-6 records and D-3 carries projections only. | Demonstrate allow without approval, deny, invalid approver, accept/decline/cancel/expiry, scope or attempt drift, retry reapproval, timeout, output cap, and untrusted-result handling. | Disable or revert the unpromoted dispatcher; after promotion, an OCR may restore the last verified new dispatcher artifact. No pending projection or partially authorized scope can execute. |
| 3 / CAND-3 | The AI-owned store adds complete lineage/checkpoint/idempotency and integrates semantic Memory/background Multitask without transferring canonical turn ownership. | Ordered replay, append-only correction, fork lineage, tenant isolation, checkpoint recovery, duplicate submission, cache loss, and CAND-1 schema evolution have exact outcomes. | Preserve prior canonical data; discard or rebuild only new projections/caches; stop recovery when ownership is ambiguous and use only a verified new schema/artifact pair. |
| 4 / CAND-4 | Instructions, profiles, skills, plugins, and MCP use one coherent extension snapshot and cannot widen permissions. | Precedence, provenance, invalid entry, source loss, stale snapshot, cross-tenant isolation, and permission non-escalation are deterministic. | Disable the registry and restore the static known-safe inventory; keep historical provenance evidence. |
| 5 / CAND-5 | Foreground/background and supported providers run on the new core and meet first-production-promotion gates. | Owned-contract conformance, stream ordering, provider behavior, background recovery, SLO thresholds, error budgets, and promotion-stop triggers are exact. | A failed first-release candidate is not promoted. After first promotion, rollback may target only the last verified new path while preserving the canonical store and owned external contract. |

## Approval And Review Checklist [Required]

- [x] Scope routing, filename, number, metadata, and central index row are correct.
- [x] Every Trello source has a captured baseline, acceptance outcome, and last-checked date.
- [x] Every functional capability cites captured requirement IDs, and every stated behavior traces to those cited baselines.
- [x] Data Model Design is triggered and ownership, lifecycle, sensitivity, relationships, and invariants are populated.
- [x] Every architecture component has a responsibility, conceptual inputs and outputs, dependencies, and cited accepted constraints; the required Mermaid architecture diagram covers every component ID, boundary, dependency, and applicable conceptual flow and agrees with the table.
- [x] Every triggered control or interaction section includes its required Mermaid diagram and structured table; the diagram covers every declared flow ID plus applicable ordering or transitions, branches, feedback, failure, and recovery, and agrees with the table. Each untriggered section records `N/A — <reason>`.
- [x] UI is not in scope and the Figma trigger is explicitly assessed as not applicable.
- [x] Cross-cutting concerns, risks, and assumptions are documented with their treatment, and every material question is resolved.
- [x] Traceability connects every requirement to capabilities and ADR task candidates or records why no runtime candidate applies.
- [x] Task candidates contain outcomes and boundaries but no source-file or executable implementation design.
- [x] Every `Selected` or `Complete` candidate has an exact reciprocal ADR path; CAND-1 is `Complete` through `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`, whose Architecture Source points back to this ADD and candidate ID.
- [x] Every required section is complete; every conditional trigger is assessed and completed or marked `N/A — <reason>`; optional content is complete.
- [x] An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded before `Current`; no Approval Context Revision is recorded because no immutable revision yet represents the approved uncommitted content.

## Archival [Conditionally Required — Design Status is `Deprecated` or `Superseded`]

This section is inactive because Design Status is `Current`. When triggered:

- [ ] Confirm every candidate is `Deferred` or `Complete`, no linked ADR has a non-terminal Implementation Status, and all reciprocal paths are current.
- [ ] Move it to `archive/ADD-0001-ai-service-codex-alignment.md` under this architecture root.
- [ ] Update all ADR, ADD, code, documentation, and task-candidate references to its final path.
- [ ] For supersession, set reciprocal `Supersedes` and `Superseded By` paths.
- [ ] Update its single row in `docs/architecture/INDEX.md`; never delete the row.
- [ ] Confirm no non-archived ADD/ADR or governed marker still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-10 | Created the Draft ADD from Trello card 4WI4sszw, pinned predecessor and OpenAI Codex evidence, and defined the target boundary and migration candidates. | Codex |
| 2026-08-11 | Added the required architecture, control-flow, and interaction-flow Mermaid diagrams; restored the diagram review checks; clarified the non-Trello R-2 source; and recorded @linhai as the reviewer confirming Q-2. | Codex |
| 2026-08-11 | Resolved lifecycle, approval authority, exact-attempt scope, append-only correction, CAND-1 storage, and migration-coexistence conflicts; aligned source-loss, durability, retry metadata, cancellation semantics, contract direction, audit retention, and C-1/C-7 interaction in both language versions. | Codex |
| 2026-08-11 | Assigned foreground orphan-turn liveness to C-2/C-6 fenced leases and reconciliation, fenced stale-owner tool dispatch and result commitment, added CAND-1/CAND-2 crash/expiry/fencing checks, and aligned migration preconditions to ADR acceptance. | Codex |
| 2026-08-10 | Approved by @linhai in the active review conversation; recorded approval metadata and the informational, non-binding Approval Context Revision `541598139e4903942b309ccb075b46473b117f7f`, and set Design Status to `Current`. | @kimi |
| 2026-08-11 | Selected CAND-1 through the Proposed, Not Started project Full ADR at `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` and synchronized the reciprocal path and review checklist. | @codex |
| 2026-08-11 | Approval-invalidating revision at 2026-08-11T09:48:15+08:00 replaced the side-by-side predecessor migration, legacy compatibility, shared-history, and route-back model with a greenfield implementation model. Preserved prior approval history: Approver `@linhai`, Approval Time `2026-08-10T17:24:17Z`, Approval Evidence `Approve`, Approval Context Revision `541598139e4903942b309ccb075b46473b117f7f`; reset Design Status to `Draft` pending reapproval. | @codex |
| 2026-08-11 | Reapproved the greenfield revision after the human approver self-declared `@linhai`, identified ADD-0001, and responded with exact `Approve`; recorded Approval Time `2026-08-11T10:37:34+08:00` and returned Design Status to `Current`. No Approval Context Revision is recorded because the approved content is not yet represented by an immutable commit. | @linhai |
| 2026-08-11 | Synchronized CAND-1 evidence after `@linhai` accepted its linked ADR at `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; candidate remains `Selected` while the ADR is `Accepted`, `Not Started`. | @codex |
| 2026-08-11 | Synchronized CAND-1 after the linked ADR returned to `Proposed`, `Not Started` for the approval-invalidating addition of its mandatory first-service Scope Routing deliverable. | @codex |
| 2026-08-11 | Synchronized CAND-1 after `@linhai` reapproved the Scope Routing revision of `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; the linked ADR is `Accepted`, `Not Started`. | @codex |
| 2026-08-11 | Synchronized CAND-1 when the linked Accepted ADR entered `In Progress` for its test-first T-1 implementation. | @codex |
| 2026-08-11 | Synchronized CAND-1 after the linked ADR returned to `Proposed`, `Not Started` to add omitted maintained Rust and generated lock paths before any governed build. | @codex |
| 2026-08-11 | Synchronized CAND-1 after `@linhai` reapproved the complete maintained-path scope and the linked ADR entered `Accepted`, `In Progress`. | @codex |
| 2026-08-11 | Synchronized CAND-1 to `Complete` after linked ADR-0001 reached `Accepted`, `Complete`; source commit `08cc1b3` satisfies AC-12's runtime precondition and all 14 ADR acceptance checks pass. | @codex |
| 2026-08-11 | Added review-correction evidence commit `56073a0` for CAND-1 concurrency, incremental streaming, provider-failure terminalization, append-deadline, and lease-worker wiring; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added second review-correction evidence commit `df49b69` for CAND-1 in-band stream failure, idle interrupt polling, nullable usage decoding, synchronous failure mapping, payload/UTF-8 validation, and heartbeat retry; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added third review-correction evidence commit `11b5ea2` for CAND-1 durability recovery ownership, subject isolation, provider-history delta coalescing, and complete JSON input/output escaping; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added fourth review-correction evidence commit `fe3beb9` for CAND-1 HTTPS-only provider configuration, runtime problem correlation, interrupt/completion arbitration, and the live 64-item fail-closed limit; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added fifth review-correction evidence commit `a7258bc` for every-provider-terminal interrupt arbitration, one bounded PostgreSQL append operation, and SSE terminal consistency; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added sixth review-correction evidence commit `a7b6faa` for serialized-payload accounting, provider-pump cancellation, and non-blocking renewal-guard shutdown; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
| 2026-08-11 | Added seventh review-correction evidence commit `31ef43f` for interruptible provider request establishment and a 1-MiB unterminated-frame cap; the candidate remains `Complete` and its accepted outcome and scope are unchanged. | @codex |
