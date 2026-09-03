# ADR-0015: Local SonarQube Feature Completion Gate

## Metadata [Required]

- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Date**: 2026-09-03
- **Author**: @zcode
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is not `Rejected`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is not `Rejected`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is not `Rejected`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is not `Deprecated` or `Superseded`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is not `Deprecated` or `Superseded`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is not `Deprecated` or `Superseded`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is not `Deprecated` or `Superseded`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is not `Blocked`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is not `Blocked`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is not `Blocked`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is not `Blocked`
- **Related [Optional]**: `docs/adr/archive/ADR-0006-one-time-local-sonarqube-baseline-analysis.md`; PR #13; approval comment `https://github.com/hailingu/koduck/pull/13#issuecomment-5519233970`
- **Architecture Source [Conditionally Required — product demand]**: N/A — repository-governance change requested by the repository owner, not derived from product demand
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

A local SonarQube instance serves the project `koduck` at
`http://localhost:9000/dashboard?id=koduck`; the archived ADR-0006 recorded
one accepted baseline analysis through it at `0.0%` coverage. The repository
owner wants every source-code feature to face a recurring local quality gate
before it can be reported complete, instead of relying on one-time baselines.

Automatic review of PR #13 established four binding facts about any such gate:

1. A repository-wide completion policy is normative governance: without an
   Accepted Full ADR, the gate is unauthorized and cannot validly govern
   subsequent features (ADR-0006 is explicitly one-time and authorizes
   nothing recurring).
2. The repository defines no canonical, non-interactive scanner workflow —
   no Scope Routing row, scanner configuration, or executable wrapper — so a
   mandatory scan would force developers to improvise inputs and produce
   incomparable analyses.
3. No routed coverage-generation or scanner-import workflow exists, so a
   mandatory `new_coverage ≥ 80%` threshold would turn the documented 0.0%
   baseline into an unconditional completion blocker.
4. SonarQube computes `new_coverage` over the project's configured New Code
   period, not automatically over one feature's diff, so an unpinned baseline
   lets unrelated changes distort the metric.

This ADR therefore authorizes the gate in a routed, conditional form: the
gate text lives in `AGENTS.md`, and it becomes mandatory for a path only when
that path's Scope Routing row records a canonical scanner workflow — with a
pinned New Code baseline — established through an accepted record.

## Scope [Required]

In scope:

- The `Local SonarQube Feature Completion Gate` section of `AGENTS.md`,
  including its routing precondition, evidence and secret-handling rules, and
  the conditional `new_coverage ≥ 80%` threshold with its base-revision
  binding.
- The two cross-references that route agents to the gate: Execution Workflow
  step 10 and the Verification And Completion Evidence completion summary.
- The `docs/adr/INDEX.md` row for this ADR.

Out of scope:

- Any scanner installation, configuration file, wrapper script, or CI wiring
  for an actual scanner workflow; each such workflow is a separate governed
  change recorded in the affected path's Scope Routing row.
- Any change to the existing CI checks, the governance validator's behavior,
  or SonarQube server configuration.
- Per-service variants of the gate.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Owner wants an enforced quality gate now, but the repository lacks the reproducible scanner and coverage workflow a mandatory gate presupposes | An unconditional gate would block every feature on improvised or missing tooling | The gate is authorized as routed and conditional: mandatory only for paths whose Scope Routing row records the canonical workflow |
| TN-2 | A single repository-wide gate text versus per-path scanner realities (Rust vs Node toolchains have different coverage and scanner mechanics) | One hardcoded invocation would be wrong for some paths | The gate defines the contract; each path records its own canonical workflow and New Code baseline |

### Constraints [Required]

- `AGENTS.md` remains the routing document; this ADR authorizes the policy and
  is cited from the gate section.
- The gate supplements — never replaces — every focused, routed, acceptance,
  and CI check, and waives no authorization required for the analysis
  operation.
- `KODUCK_SONAR_TOKEN` handling rules in the gate section are binding: never
  print the token, place it in command arguments or repository files, include
  it in captured output, or persist it outside the existing shell
  configuration.
- Until a path records the canonical workflow, the gate is advisory for that
  path and completion relies on the path's routed checks.
- A canonical workflow MUST analyze an immutable source: a clean detached
  worktree at the exact feature revision, or an equivalent verification that
  the uploaded content matches that revision, and when the workflow generates
  and imports coverage it MUST bind coverage generation and report import to
  the same immutable feature revision as the scan, so uncommitted worktree
  edits, foreign checkouts, or stale reports cannot make evidence claim the
  wrong revision.
- The 80% threshold applies to the new lines the analyzed feature diff
  introduces; when that diff introduces no coverable new lines, an absent
  `new_coverage` measure with a `Passed` Quality Gate satisfies the coverage
  clause instead of the numeric threshold.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Should the 80% threshold be per-path configurable? | @linhai | 2026-09-03 | Resolved | No — one repository-wide threshold is fixed here; a path that needs a different threshold must obtain its own accepted decision record amending this one. |
| Q-2 | What makes a recorded workflow canonical? | @zcode | 2026-09-03 | Resolved | The affected path's Scope Routing row names the exact non-interactive scanner command, its source and exclusion inputs, its terminal-state wait behavior, the New Code baseline definition, and an immutable source guarantee — a clean detached worktree at the exact feature revision or an equivalent content verification, with coverage generation and report import bound to the same immutable feature revision as the scan, following the clean-checkout practice of `docs/adr/ocr/archive/OCR-0011-local-sonarqube-validator-structural-parsing-reliability-verification.md` — established through an accepted record. |

## Decision Drivers [Required]

1. **Governance authorization**: repository-wide completion policy requires an
   Accepted Full ADR before it can bind agent behavior.
2. **Reproducibility**: gate evidence must be comparable across features and
   agents, which requires a canonical workflow rather than improvised scanner
   inputs.
3. **Non-blocking defaults**: the gate must not make completion impossible for
   paths that lack tooling today.
4. **Metric validity**: a coverage threshold is only meaningful when the New
   Code baseline isolates the analyzed feature diff.

## Options Considered [Required]

### Option: Unconditional gate

`AGENTS.md` requires every source-code feature to submit a SonarQube analysis
with a `Passed` Quality Gate and `new_coverage ≥ 80%` before completion.

Pros:

- Simple, uniform, immediately enforced.

Cons:

- Unauthorized without this ADR; blocks all completion given the absent
  scanner and coverage workflows; unpinned New Code baseline distorts the
  metric.

### Option: No gate

Keep relying on routed checks and CI only.

Pros:

- No new governance surface.

Cons:

- Rejects the owner's stated goal of a local quality gate before completion.

### Option: Routed conditional gate (selected)

`AGENTS.md` carries the gate; it binds a path only after that path's Scope
Routing row records a canonical scanner workflow with a pinned New Code
baseline, established through an accepted record.

Pros:

- Authorized, reproducible, non-blocking for un-routed paths, and metrically
  valid where it applies.

Cons:

- The gate enforces nothing until each path records its workflow.

## Decision [Required]

**Selected option**: Routed conditional gate.

**Rationale**: It is the only option that satisfies all four drivers at once:
this Accepted record supplies the authorization, the routing precondition
forces reproducibility before enforcement, advisory status for un-routed
paths removes the impossible-blocker failure mode, and the baseline binding
keeps the threshold meaningful. The main cost — no enforcement until workflows
are recorded — is the correct sequencing rather than a defect, because the
alternatives enforce chaos or enforce nothing.

### Consequences [Required]

Positive:

- A single authorized policy text; per-path enforcement that cannot fire
  before its tooling exists; comparable evidence once workflows land.

Negative:

- Paths without recorded workflows get no additional gate today.
- Two governance artifacts must stay synchronized (gate text cites this ADR).

Mitigations:

- Each path can record its workflow through one small accepted change to its
  Scope Routing row; the gate's own conditions enumerate exactly what must be
  recorded.
- This ADR is the single authorization citation; the gate section carries no
  independent authority.

## Implementation Plan [Required]

**Complete task outcome**: `AGENTS.md` carries an authorized, routed, and
conditional Local SonarQube Feature Completion Gate whose mandatory effect on
any path begins only when that path's Scope Routing row records a canonical
scanner workflow with a pinned New Code baseline, with the gate section, its
two cross-references, and the index row for this ADR landed in one
documentation change.

**Primary implementation boundary**: N/A — documentation-only governance
change; no source, configuration, build, or runtime boundary is modified.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Land the gate policy and its routing | `AGENTS.md` gate section citing this ADR, Execution Workflow step 10 cross-reference, Verification And Completion Evidence cross-reference, and the `docs/adr/INDEX.md` row for this ADR | Not Started | Pending — the gate text and citations are staged in PR #13 and complete with this record's reapproval. Historical evidence: the `### Local SonarQube Feature Completion Gate` section in `AGENTS.md` opens with the citation of `docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md`, both cross-references route only paths whose Scope Routing row records the workflow, and the index row records this ADR; verified by AC-1 through AC-4 |

**Affected paths**: `AGENTS.md`, `docs/adr/INDEX.md`,
`docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

N/A — documentation-only governance change; no source or configuration
implementation exists to anchor. The normative text anchors are the
`### Local SonarQube Feature Completion Gate` heading in `AGENTS.md`, the
`Local SonarQube Feature Completion Gate` cross-reference in Execution
Workflow step 10, and the completion summary bullet in the Verification And
Completion Evidence section of `AGENTS.md`.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: N/A — the gate is newly created and advisory
until routed; no existing behavior changes, and reverting the documentation
change removes the gate with no runtime effect.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule of `docs/development/software-engineering-standard.md`
is exceeded or waived; no maintained source or configuration is modified.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

N/A — documentation-only governance change with no public or internal code
contract; the normative clauses of the gate section are verified through the
deterministic document inspections in the acceptance checks below.

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

N/A — documentation-only governance change with no runtime concurrency,
timeout, cancellation, resource-bound, or framework execution surface; the
baseline risk dimensions have no executable scenario to exercise. The
operational risks of the gate itself (unauthorized policy, improvised scans,
impossible blockers, distorted baselines, secret leakage, missing evidence)
are resolved by this record's decision and must be demonstrated by AC-1
through AC-8 before completion is recorded.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Every mandatory clause of the `AGENTS.md` gate section — scan submission, Quality Gate, and the 80% threshold — is conditioned on the affected path's Scope Routing row recording the canonical scanner workflow with its immutable source guarantee extended to coverage generation and report import, the 80% clause additionally requires coverage import plus a base revision or equivalent New Code definition matching the feature diff, and a diff with no coverable new lines is satisfied by an absent measure with a `Passed` Quality Gate | The `AGENTS.md` revision carrying the gate | Deterministic inspection of the `### Local SonarQube Feature Completion Gate` section | Each mandatory clause textually opens with the routing precondition or the coverage-import precondition; the workflow definition requires an immutable clean-checkout or equivalent content verification with coverage generation and report import bound to the same revision; no unconditional completion blocker remains; the zero-coverable-lines satisfaction path is present; the section cites `docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md` | Inspection note quoting the conditioning phrases, the zero-coverable-lines clause, and the citation | Not Started | Historical evidence from the pre-invalidation revision — re-execute after reapproval. Inspection of the section in this pull request's `AGENTS.md`: the gate opens with "Authorized by `docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md`", the applicability clause opens "This gate applies to a source-code feature only when the affected path's Scope Routing row records a canonical, non-interactive scanner workflow", the threshold clause opens with the coverage-import precondition and requires the base-revision-or-New-Code-definition binding, and the zero-coverable-lines clause states "When the pinned feature diff introduces no coverable new lines ... that absent value with a `Passed` Quality Gate satisfies the coverage clause" |
| AC-2 | T-1 | Both cross-references route only routed paths to the gate | The same `AGENTS.md` revision | Deterministic inspection of Execution Workflow step 10 and the Verification And Completion Evidence completion summary | Both sites state that the gate applies only to a path whose Scope Routing row records the workflow; neither states an unconditional coverage or scan requirement | Inspection note quoting both sites | Not Started | Historical evidence from the pre-invalidation revision — re-execute after reapproval. Step 10 reads "For a source-code feature whose path's Scope Routing row records the Local SonarQube scanner workflow, also satisfy the Local SonarQube Feature Completion Gate"; the completion summary opens "A source-code feature on a path whose Scope Routing row records the Local SonarQube scanner workflow additionally requires ..." and carries the same missing-value-as-evidence rule |
| AC-3 | T-1 | The governance validator passes for the whole repository with the gate and this ADR present | Repository at the change revision | `npm run validate --prefix tools/governance-validator` | Command output ends with `Governance validation passed.` | Command output | Not Started | Historical evidence — re-execute on the reapproved revision |
| AC-4 | T-1 | The validator test suite passes | Repository at the change revision | `npm test --prefix tools/governance-validator` | Summary reports 184 tests with 0 failed | Command summary | Not Started | Historical evidence — re-execute on the reapproved revision |
| AC-5 | T-1 | The gate's secret-handling bullet names `KODUCK_SONAR_TOKEN` from the process environment initialized by `~/.zshrc` and states every containment prohibition — never print the token, never place it in command arguments, never place it in repository files, never include it in captured output, never persist it outside the existing shell configuration | The `AGENTS.md` revision carrying the gate | Deterministic inspection of the gate's secret-handling bullet | The bullet contains all five prohibitions verbatim in substance and names the token source; no prohibition is weakened or omitted | Inspection note quoting the bullet | Not Started | Pending |
| AC-6 | T-1 | The gate's evidence bullet requires reporting the compute-task or analysis identifier, terminal processing result, Quality Gate result, the actual `new_coverage` value when the workflow provides one, and the project dashboard URL, scoped to where the gate applies | The `AGENTS.md` revision carrying the gate | Deterministic inspection of the gate's evidence-reporting bullet | The bullet enumerates all five evidence items and scopes them to paths where the gate applies | Inspection note quoting the bullet | Not Started | Pending |
| AC-7 | T-1 | Every workflow-canonicality and authorization-preservation clause of the gate remains present: the routing precondition's workflow definition enumerates the exact scanner command, the source and exclusion inputs, the terminal-state wait behavior, and establishment through an accepted decision record; the submission clause restricts analyses to the recorded workflow with no improvised scanner parameters; and the supplementation clause preserves every focused, routed, acceptance, and CI check and waives no authorization required for the analysis operation | The `AGENTS.md` revision carrying the gate | Deterministic inspection of the `### Local SonarQube Feature Completion Gate` section | The routing-precondition bullet names "the exact scanner command, its source and exclusion inputs, its terminal-state wait behavior" and closes its workflow definition with "established through an accepted decision record"; the submission bullet states analysis is submitted "only through the recorded workflow, never through improvised scanner parameters" and that improvised source, exclusion, or wait inputs produce "incomparable analyses and invalid evidence"; the same bullet states the gate "does not replace any of them or waive any authorization required for the analysis operation" | Inspection note quoting each enumerated workflow component and both authorization clauses | Not Started | Pending |
| AC-8 | T-1 | The gate binds its applicability and its reported evidence to the exact analysis target: the applicability clause identifies the local SonarQube project `koduck` at the exact dashboard URL, and the evidence clause scopes every reported item to the exact analyzed revision | The `AGENTS.md` revision carrying the gate | Deterministic inspection of the `### Local SonarQube Feature Completion Gate` section | The applicability bullet states the workflow is recorded "for the local SonarQube project `koduck` at `http://localhost:9000/dashboard?id=koduck`"; the evidence bullet opens "Report non-secret evidence for the exact analyzed revision where the gate applies" before enumerating its items, so reported evidence cannot certify an unrelated project or revision | Inspection note quoting the project identity and the exact-revision qualifier | Not Started | Pending |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Not Started | Pending — the prior approval was invalidated by the immutable-source amendment and is preserved in the Change Log; a new Approve is required |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending — re-execute T-1 and AC-1 through AC-8 after reapproval |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — governance ADR not derived from product demand; no ADD task candidate applies | Not applicable | N/A — no ADD task candidate exists for this governance record | N/A |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Not Started | Pending — re-review after reapproval. Prior evidence: all `[Required]` sections complete; Stable Implementation Touchpoints, Contract-To-Check Traceability, Risk Coverage Matrix, Migration, and Engineering Exceptions record specific `N/A — <reason>` values with their documentation-only justification; the optional Approval Context Revision field is removed and Related is complete |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Not Started | Pending — re-review after reapproval. Prior evidence covers only AC-1 through AC-4, the checks that existed and were recorded `Pass` at the pre-invalidation revision: each named T-1, quoted exact preconditions, deterministic inspection or command methods, exact expected results, and captured evidence. AC-5 through AC-8 were added after the invalidation and await their first review after reapproval; each follows that same form |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering rule is exceeded or waived | Not applicable | N/A — documentation-only change touches no maintained source or configuration | N/A |
| A-7 | Contract and baseline risks covered, when applicable | N/A — documentation-only change; no code contract and no executable risk surface | Not applicable | N/A — documentation-only governance change; the gate's operational risks (unauthorized policy, improvised scans, impossible blockers, distorted baselines, secret leakage, missing evidence) are resolved by this record's decision and must be demonstrated by AC-1 through AC-8 before completion is recorded | N/A |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Not Started | Pending — re-run on the reapproved revision |

## Supporting Notes [Optional]

The gate's advisory-until-routed design mirrors how this ADR itself was
shaped: automatic review of PR #13 rejected the unconditional form three
times (coverage workflow, scanner route, authorization) before this routed
conditional form. Establishing a path's scanner workflow is intentionally a
separate future change so that its scanner command, exclusions, wait
behavior, and New Code baseline can be verified against real tooling rather
than invented here.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-0015-local-sonarqube-feature-completion-gate.md`
      under this same ADR root (`docs/adr/archive/`).
- [ ] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed code was deleted.
- [ ] If Decision Status is `Superseded`, set the replacement record's
      `Supersedes` field and this record's `Superseded By` field to each other's
      final repository-relative path.
- [ ] If no record supersedes this one, retain `Superseded By: None`.
- [ ] Update this record's single row in `docs/adr/INDEX.md` with the archived
      path, scope, and final status.
- [ ] Confirm no ADR or OCR outside an `archive/` directory, and no code marker, still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-03 | Drafted as the authorized, routed conditional form of the Local SonarQube Feature Completion Gate after PR #13 automatic review established the coverage-workflow, scanner-route, authorization, and New Code baseline constraints; proposed for approval. | @zcode |
| 2026-09-03 | Approved by `@linhai` with exact `Approve` at `2026-09-03T02:08:08Z` in PR #13 comment 5519233970, which identifies this record's repository-relative path; Decision Status recorded `Accepted`, T-1 and AC-1 through AC-4 recorded `Complete`/`Pass` with evidence, completing the record in the same change that lands the gate. | @zcode |
| 2026-09-03 | Approval invalidated at `2026-09-03T10:21:58+08:00` (committer time of the amending revision `7195e7a1fe7f6047ed16ed09283cb6aca6ed2a5d`): automatic review `5097094674` required the canonical workflow to guarantee an immutable source (clean detached worktree at the exact revision or equivalent content verification), changing the resolved question Q-2, Constraints, and AC-1. Per the approval-invalidating change rules, the prior Approver `@linhai`, Approval Time `2026-09-03T02:08:08Z`, and Approval Evidence `Approve` are preserved here; active approval fields are reset to `Pending — reapproval required`, Decision Status to `Proposed`, and Implementation Status to `Not Started` with statuses reset. | @zcode |
| 2026-09-03 | Automatic review `5097564791` (PR #13 comment `3920860246`) found that AC-1 through AC-6 could all pass even if the `AGENTS.md` gate lost the exact scanner command, source and exclusion inputs, terminal-state wait behavior, accepted-record provenance, or the clauses preserving operational authorization; added AC-7 to inspect those clauses deterministically and updated every AC reference. The record stays `Proposed`; no approval is in force to invalidate. | @zcode |
| 2026-09-03 | Automatic review `5097646359` (PR #13 comments `3920934001` and `3920934005`) found that no check asserted the gate's analysis-target identity (project `koduck` at `http://localhost:9000/dashboard?id=koduck`) or the exact-revision binding of reported evidence, and that A-5 claimed pre-invalidation prior evidence for AC-5 and AC-6 although those checks were added after the invalidation; added AC-8 for target identity and revision binding, and corrected A-5's prior-evidence scope to AC-1 through AC-4 with AC-5 through AC-8 awaiting their first review after reapproval. The record stays `Proposed`; no approval is in force to invalidate. | @zcode |
