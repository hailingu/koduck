# Software Engineering Standard

**Applies to**: all maintained production and test source code in this
repository. Read this file together with every matching language standard.

**Last reviewed**: 2026-08-10

## Purpose And Precedence

This standard governs software structure and maintainability across languages.
Language standards define how these rules map to native packages, modules,
types, components, interfaces, and tools. A language standard may narrow this
baseline but must not weaken or contradict it.

Accepted decisions, public contracts, security and accessibility requirements,
the applicable Figma design, and platform constraints take precedence when they
bind a design. Record and resolve a conflict instead of silently choosing one
rule over another.

## Size And Complexity Guardrails

Size is a review signal, not a substitute for judging cohesion. Crossing a
review threshold requires an explicit decomposition review. Crossing an
exception limit requires a documented engineering exception before the change
is accepted.

| Unit | Decomposition-review threshold | Engineering-exception limit |
| --- | --- | --- |
| Maintained production source file | More than 400 physical lines | More than 800 physical lines |
| Maintained test source file | More than 600 physical lines | More than 1,200 physical lines |
| Function, method, closure, or equivalent executable unit | More than 60 physical lines | More than 120 physical lines |
| Cyclomatic complexity, when measured by configured tooling | More than 10 | More than 20 |
| Executable nesting depth | More than 4 levels | More than 6 levels |

Apply the guardrails as follows:

- Count the physical span, including comments and documentation, because that
  is the amount a reviewer must navigate. Use configured tool output for
  complexity when available; do not add a dependency solely to obtain a metric.
- Generated, vendored, lock, machine-produced schema, snapshot, fixture-data,
  and immutable migration-history files are excluded from the numeric limits.
  Their source or generating workflow remains subject to review.
- When no configured tool measures cyclomatic complexity, record
  `N/A — no configured complexity tool` and use executable-unit line span plus
  nesting depth as the required substitute signals. Tool absence never means
  that the complexity threshold passed.
- Declarative registries, protocol declarations, UI composition, and exhaustive
  test tables are not automatically exempt. They may remain large when the
  decomposition review demonstrates one cohesive responsibility.
- Crossing a review threshold does not require a mechanical split. Record why
  the unit is cohesive and why extraction would worsen coupling, readability,
  ordering, or lifecycle safety.
- Do not satisfy a limit by moving unrelated code into a generic helper,
  creating pass-through wrappers, or splitting one operation into files that
  must always be read and changed together.

## Responsibility And Module Boundaries

- A module, package, feature, or component must own one coherent capability and
  have one primary reason to change. Name it after that responsibility rather
  than an incidental implementation detail.
- Organize services and applications by feature or domain at their main
  boundary. Use technical layers inside a feature when separating them creates
  a meaningful dependency, failure, ownership, or test boundary.
- Separate transport or UI delivery, application orchestration, domain policy,
  persistence, and external-provider integration when they vary, fail, scale,
  or are tested independently. Small programs may keep them together while
  those forces do not exist.
- Keep the public surface minimal and intentional. Internal implementation
  details must not become public merely to make tests or imports convenient.
- A new module must have a clear owner, inputs, outputs, invariants, and error
  behavior. Public modules and declarations require intent-bearing
  documentation before their implementation bodies.
- Generic `utils`, `helpers`, `common`, `shared`, or `misc` dumping grounds are
  prohibited. Shared code needs a specific capability name and must satisfy the
  extraction criteria in
  [Abstractions And Design Patterns](#abstractions-and-design-patterns).
- Prefer colocating code that changes together. Split code when responsibilities
  or change lifecycles differ, not merely because a directory looks large.

## Dependency Design

- Dependencies point from delivery and infrastructure adapters toward stable
  application or domain contracts. Core business rules must not depend on Web,
  UI, database, provider, serialization, or framework details.
- Cyclic dependencies between maintained modules or packages are prohibited.
  Resolve a cycle by clarifying ownership, extracting a stable contract, or
  moving shared policy to the module that owns it.
- Cross-module calls must use the owning module's public API. Do not import its
  internal persistence models, framework objects, mutable state, or private
  helpers.
- Translate external request, response, database, and provider types at the
  boundary. Do not let them become the repository-wide domain model by
  convenience.
- Keep dependency direction visible in names and layout. A boundary that exists
  only by convention but is routinely bypassed is not an effective boundary.

## Abstractions And Design Patterns

- Start with the simplest direct design that preserves the required boundary.
  Introduce an abstraction only for demonstrated variation, independent
  lifecycle, external I/O, reusable policy, or a necessary test seam.
- Interfaces, protocols, and traits belong at the boundary that consumes the
  behavior. Do not create one interface per concrete type or a factory for a
  single direct construction path without an identified variation.
- Before applying a named design pattern, identify the recurring problem, the
  forces it resolves, the expected variation, and the simpler alternative that
  was considered. The pattern is an implementation tool, not a target
  architecture.
- Prefer composition and explicit delegation over inheritance. Inheritance is
  appropriate only for a genuine substitutable relationship with preserved
  invariants, not for code reuse alone.
- Use dependency injection explicitly through constructors, parameters, or
  language-native environment mechanisms. Service locators and mutable global
  singletons are prohibited unless an approved exception defines their bounded
  lifetime and removal plan.
- Do not generalize code after a single example. Extract shared behavior when
  at least two real consumers have the same semantics and expected evolution.
  A consumer-owned external boundary may be defined before a second
  implementation exists when it prevents external types or semantics from
  leaking into the core. Similar syntax with different business meaning must
  remain separate.

### Pattern Selection Guide

Use this table to start a design discussion. A named pattern is justified by
the stated problem and forces, not by matching a class diagram mechanically.

| Pattern or approach | Appropriate problem | Do not use it merely to |
| --- | --- | --- |
| Adapter / anti-corruption layer | Translate an external API, framework, provider, storage, or legacy model into an owned contract. | Rename fields while still leaking external semantics throughout the core. |
| Strategy | Select between behaviorally meaningful algorithms or providers behind one stable consumer-owned contract. | Hide one implementation or replace a simple conditional whose variants do not evolve independently. |
| Explicit state machine | Model a finite lifecycle with guarded transitions, invalid states, retries, cancellation, or terminal outcomes. | Distribute state checks across handlers without one transition owner. |
| Repository | Give domain logic collection-like access to persisted aggregates while hiding storage mechanics. | Wrap every CRUD table or forward ORM calls without domain semantics. |
| Domain/application service | Own policy or orchestration that does not naturally belong to one value or entity. | Create broad `Service` or `Manager` classes containing unrelated use cases. |
| Event / observer | Notify multiple independent consumers or cross an asynchronous ownership boundary with defined delivery semantics. | Replace a direct call when ordering, failure, duplication, and ownership are unspecified. |
| Middleware / decorator | Apply ordered cross-cutting boundary behavior such as authentication, tracing, retries, or rate limits. | Hide core business rules or create an execution order that reviewers cannot trace. |
| Factory / builder | Centralize construction that varies by type, enforces multiple invariants, or has a staged configuration. | Avoid a clear constructor or create indirection for a single fixed type. |
| Facade | Present a small stable API over a complex subsystem owned behind that boundary. | Create a pass-through layer that adds no ownership, translation, policy, or compatibility value. |
| Vertical slice | Keep one use case's delivery, application logic, and tests together while respecting inward dependencies. | Duplicate shared domain policy or bypass another feature's public API. |
| Ports and adapters | Isolate a substantial core from multiple volatile delivery, persistence, or provider technologies. | Add layers to a small script or simple CRUD feature with no independent domain behavior. |

## State, Concurrency, And Failure Boundaries

- Give every mutable state value one clear owner. State transitions must be
  explicit, validated, and observable at the boundary where failure matters.
- Make side effects visible in APIs and keep them at controlled boundaries.
  Domain calculations should be deterministic where practical.
- Define cancellation, timeout, retry, idempotency, and partial-success behavior
  before adding concurrent or distributed execution. A retry must not duplicate
  a non-idempotent effect.
- Handle errors at the layer that can add context or choose recovery. Preserve
  the original cause and use the project's structured diagnostics; do not log
  and rethrow the same failure at every layer.
- Follow the trust-boundary validation requirements in the Security section of
  the root [`AGENTS.md`](../../AGENTS.md). After validation, maintain valid
  internal types and invariants rather than propagating uncertain external
  state through the core.

## Testing And Change Design

- Test observable behavior and stable contracts. Avoid tests coupled to private
  call order, framework internals, or incidental data structures unless that
  detail is itself the contract.
- Keep unit tests near their owning module according to language convention.
  Put integration and contract tests at the boundary they exercise.
- A bug fix should add the smallest regression test that fails for the reported
  behavior and passes for the corrected behavior when reproduction is feasible.
- A module split is complete only when its tests, ownership, public API, and
  dependency direction reflect the new boundary; moving lines alone is not a
  structural improvement.
- Reuse an existing component, client, schema, error type, test fixture, or
  helper when it has the same ownership and semantics. Do not create a parallel
  abstraction solely to avoid understanding the existing one.

## Incremental Adoption And Exceptions

New code must comply immediately. Existing code is assessed when it is
materially changed. Do not expand an unrelated task solely to remediate an old
threshold unless the approved scope includes that refactor; record the observed
debt, its affected path or symbol, and the reason it remains out of scope in the
governing Full ADR's `Supporting Notes` or Lightweight ADR's `Notes` section.

### Exception Authority And Storage

An engineering exception is approval-sensitive content in the ADR that governs
the affected source change. An OCR cannot authorize source changes and cannot
carry a source-code engineering exception. A pull-request description, review
comment, label, or verification note may link to or repeat an exception for
review convenience but cannot authorize it.

Record each exception in a dedicated `Engineering Exceptions` subsection under
the governing ADR's `Implementation Plan`. Use the heading
`### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]`
and one row per affected unit with these fields:

1. The exact rule and affected path or symbol.
2. The measured value or structural condition.
3. Why the unit remains cohesive or why compliance is unsafe now.
4. The risks created by retaining it.
5. Compensating controls, including focused tests or ownership restrictions.
6. One accountable owner.
7. A removal or review date, or a specific reason the exception is permanent.
8. The verification evidence that demonstrates the compensating controls.

The exception becomes effective only after that ADR is `Accepted`. If an
exception is discovered after approval, update the ADR and complete the
approval-invalidating change and reapproval workflow before a material change
introduces or retains the exceptional source.

Use the existing governed-file marker required by the root
[`AGENTS.md`](../../AGENTS.md) to link the affected source to its ADR.
Generated, vendored, lock, binary, and comment-free formats remain discoverable
through the ADR's affected-files evidence instead of receiving a new marker
format. The marker leads to the exception subsection, and
`docs/adr/INDEX.md` provides the repository-wide record catalog; no separate
exception registry is maintained.

### Exception Lifecycle

Revisit the exception whenever the affected unit is materially changed. An
expired exception does not authorize further growth. Changing its rule, scope,
risk, controls, owner, review date, or permanent rationale requires the
governing ADR's applicable update and reapproval workflow.

When a later accepted task removes the exceptional condition, remove or update
the governed-file marker as required by that task's ADR and record remediation
evidence there. Preserve the original ADR exception as historical evidence
rather than rewriting its approved decision trail.

## Before Writing Code

Read this file and every matching language or platform standard in full. Then
inspect the affected module and answer these questions before implementation:

- Which module owns the behavior and its invariants?
- Which responsibilities change together, and which vary independently?
- What is the permitted dependency direction?
- Is an existing internal capability semantically suitable?
- Does the design cross a review threshold or require an exception?
- What observable contract and failure behavior will the tests verify?
