# ADR-0002: Required Koduck AI CI And PostgreSQL Verification

## Metadata [Required]

- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Date**: 2026-08-12
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
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
- **Related [Optional]**: [Pull request 1](https://github.com/hailingu/koduck/pull/1)
- **Architecture Source [Conditionally Required — product demand]**: N/A — this is repository verification governance, not product demand
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

The repository now requires every Scope Routing verification command for a
source or configuration pull request to have a corresponding green required CI
check. Pull request 1 changes `koduck-ai/**`, whose routing row requires format,
strict Clippy, and all-target/all-feature tests, but the repository has no CI
workflow or required status checks.

The service also claims PostgreSQL transaction, lock, subject-ownership,
payload, and fencing behavior while its current tests inspect SQL source or use
in-memory doubles. The software-engineering standard requires production
datastore risks to be exercised through the production boundary or a
behaviorally equivalent integration harness. This decision defines one CI
verification boundary that supplies the three routed checks, executes the
PostgreSQL cases through `SqlxPostgresExecutor`, and makes the check names
required on `dev` through a separately accepted operational change.

## Scope [Required]

In scope:

- One GitHub Actions workflow for pull requests targeting `dev`, with checks
  named exactly `koduck-ai-format`,
  `koduck-ai-clippy`, and `koduck-ai-test-postgres`; the format job executes
  the routed governance commands (`npm test --prefix tools/governance-validator`
  and `npm run validate --prefix tools/governance-validator`) before its exact
  Rust format command, per the `AGENTS.md` Scope Routing row for
  `.github/workflows/koduck-ai.yml`, so either a governance failure or a
  formatting failure fails the already-required `koduck-ai-format` context
  while the exact three protected context names stay unchanged.
- Workspace package metadata and all three jobs use the repository owner's
  selected Rust 1.95 toolchain, which exceeds the Rust 1.94 minimum required
  by the committed `sqlx 0.9.0` dependency.
- One focused recovery-error regression test and the minimum
  semantics-preserving `runner.rs` conditional rewrite required for the Rust
  1.95 strict-Clippy contract; no runtime branch or error outcome may change.
- An ephemeral PostgreSQL service used by the test check and a production-boundary
  SQLx integration test covering migration, subject ownership, escaped U+0000,
  terminal arbitration, and stale-generation fencing.
- Required-status-check configuration for `dev`, applied only through an
  Accepted OCR after the workflow check names exist.
- Stable CI and database evidence recorded against the exact tested revision.

Out of scope:

- Deployment, release, artifact publication, provider live-endpoint testing,
  changes to the public REST/SSE contract, and non-`koduck-ai` build pipelines.
- A shared or long-lived PostgreSQL environment; CI uses an isolated disposable
  service with no production or user data.
- Runtime source fixes governed by ADR-0001.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | A real datastore test provides transaction evidence but introduces service startup and readiness latency. | Source inspection remains fast but cannot prove SQL, lock, or type behavior; an unbounded service wait can stall CI. | Run only the routed test job with an ephemeral PostgreSQL service, use a bounded health check, and keep unit tests independent of external state. |
| TN-2 | Repository workflow files can emit named checks, while making them required mutates GitHub repository settings. | Treating an emitted check as required would leave the review-ready gate unenforced. | Add the workflow under this ADR and apply the exact required-check setting through a separate Accepted OCR with rollback. |
| TN-3 | The committed workspace declared Rust 1.85 while `sqlx 0.9.0` requires at least Rust 1.94. | Both Clippy and test jobs fail before compilation, so the emitted checks cannot verify the routed commands. | Declare workspace `rust-version = "1.95"` and pin every CI job to the repository owner's selected Rust 1.95 toolchain; do not change or downgrade the accepted dependency lock. |
| TN-4 | Raising workspace `rust-version` to 1.95 activates Rust 1.95's `collapsible_if` guidance in strict Clippy for two nested conditionals in recovery scheduling. | The exact routed Clippy command fails even though dependency compilation is now supported. | Apply only Clippy's semantics-preserving let-chain rewrite in `runner.rs`; retain existing recovery tests and require the full routed suite to pass. |

### Constraints [Required]

- The exact routed commands remain `cargo fmt --all --check`,
  `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`, and
  `cargo test -p koduck-ai --all-targets --all-features`; the format job
  additionally executes both routed governance commands before its Rust
  format command, so governance validation is an accepted failure condition
  of the protected `koduck-ai-format` context itself.
- Root workspace package metadata and each workflow job must use Rust 1.95;
  any compiler-version change is approval-invalidating because it changes the
  supported build contract.
- The Rust 1.95 compatibility edit in `runner.rs` must preserve the existing
  `Unavailable` recovery scheduling and non-`Unavailable` error propagation.
- The required check names are exactly `koduck-ai-format`,
  `koduck-ai-clippy`, and `koduck-ai-test-postgres`; a rename is an
  approval-invalidating contract change because branch protection binds these
  identities.
- CI must use the committed `Cargo.lock` with `--locked` where Cargo accepts it
  without changing the command's verification meaning.
- PostgreSQL verification must call the production migration and
  `SqlxPostgresExecutor`; source-string assertions and in-memory executors do
  not satisfy the datastore acceptance check.
- CI credentials are disposable local service values and must not contain or
  expose repository, provider, production, or user secrets.
- The GitHub settings mutation must not occur until an OCR for the exact
  repository, branch, check names, preflight, rollback, and evidence is
  `Accepted`.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Which CI and datastore approach should fix the review gate? | @linhai | 2026-08-12 | Resolved | In the active task, @linhai confirmed the recommended three-check GitHub Actions workflow and real PostgreSQL integration approach. |
| Q-2 | Which compiler version can execute the committed dependency graph? | @linhai | 2026-08-12 | Resolved | GitHub Actions runs `31553057678` and `31554660991` show Rust 1.85.1 rejecting the committed `sqlx 0.9.0`, which requires Rust 1.94.0. The local workspace uses Rust 1.95 successfully, and `@linhai` explicitly selected Rust 1.95 for workspace metadata and CI. |

## Decision Drivers [Required]

1. **Revision-bound merge evidence**: Every routed command must be visible and
   enforceable for the exact pull-request revision.
2. **Production-boundary confidence**: PostgreSQL SQL, constraints, locking, and
   codec behavior must run against PostgreSQL rather than source inspection.
3. **Fail-closed governance**: Missing, skipped, cancelled, or failing required
   checks must prevent review-ready status.
4. **Disposable isolation**: Verification must not mutate a shared or external
   datastore or retain a promotable artifact.
5. **Truthful compiler contract**: Workspace metadata and CI must agree with
   the minimum Rust version required by the committed dependency graph.

## Options Considered [Required]

### Option: Required GitHub Actions Checks With Ephemeral PostgreSQL

Create three named workflow jobs, attach PostgreSQL only to the test job, run a
production-boundary integration test as part of the routed all-target suite,
and require all three names on `dev` through an Accepted OCR.

Pros:

- Directly satisfies the repository's routed-command and datastore-boundary
  requirements with revision-bound evidence.
- Keeps database state disposable and limits PostgreSQL cost to the test job.

Cons:

- Adds CI latency and requires one governed external settings operation.

### Option: Retain Local Commands And Source Inspection

Continue recording local command output and inspecting SQL strings without CI
or PostgreSQL.

Pros:

- Adds no workflow or service startup time.

Cons:

- Cannot satisfy the required-CI gate or prove PostgreSQL transaction, lock,
  type, migration, and constraint behavior.

### Option: Use A Shared PostgreSQL Test Environment

Run tests against a persistent team database outside each workflow execution.

Pros:

- Avoids per-run service startup.

Cons:

- Introduces credentials, cross-run interference, cleanup risk, and shared-state
  mutation outside Disposable Verification Execution.

## Decision [Required]

**Selected option**: Required GitHub Actions Checks With Ephemeral PostgreSQL

**Rationale**: This is the only option that makes every routed command an
enforceable revision-bound check while exercising the production datastore
boundary without shared infrastructure or credentials. The workflow and branch
protection setting form one CI verification boundary; the SQLx test is the
minimum supporting harness needed for that boundary to establish its claimed
PostgreSQL result.

### Consequences [Required]

Positive:

- Pull requests cannot become review-ready with missing format, lint, test, or
  PostgreSQL evidence.
- PostgreSQL migration, ownership, payload, locking, and fencing regressions are
  observable before merge.
- Check results are tied to the exact pushed revision.

Negative:

- The test job depends on PostgreSQL container availability and will run longer.
- Required-check activation and rollback need a separately approved GitHub
  repository-settings operation.
- Raising the declared Rust version from 1.85 to 1.95 drops older Rust
  toolchains, including Rust 1.94 even though it meets the dependency minimum;
  this is the repository owner's explicit toolchain selection.

Mitigations:

- Use a bounded health check and workflow timeout, isolate one disposable
  database per job, and retain no database volume or build artifact.
- Pin Rust 1.95 in workspace metadata and every job so dependency requirements,
  local expectations, and revision-bound CI agree.
- Apply and verify branch protection through an OCR that records the prior
  state and restores it exactly on failure.

## Implementation Plan [Required]

**Complete task outcome**: Pull requests targeting `dev` emit and require
`koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres` for the
exact routed format, strict-Clippy, and all-target/all-feature test commands;
the test check runs a disposable PostgreSQL service and proves migration,
subject ownership, escaped U+0000, terminal arbitration, and stale-generation
fencing through the production SQLx executor.

**Primary implementation boundary**: Repository CI verification pipeline

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Add the three revision-bound routed verification checks. | Root workspace Rust 1.95 metadata; one `.github/workflows/` workflow pinned to Rust 1.95; one focused recovery-error regression test; the minimum semantics-preserving `runner.rs` Clippy compatibility rewrite; exact format, strict-Clippy, and all-target/all-feature test commands; bounded job timeouts; no retained artifact. | Complete | Rust 1.95 compiler-contract test observed RED at the 1.85 metadata and GREEN after root plus three-job pins; recovery-error test killed the error-discarding mutation and passed the let-chain rewrite; local format, strict Clippy, and 77 all-target/all-feature tests passed under `rustc 1.95.0`. Workflow run `31556012717` for exact SHA `18f1404a8edf6bb296960dadca8553e53e6b50e5` completed all three jobs successfully. |
| T-2 | Replace source-inspection-only PostgreSQL claims with production-boundary evidence. | Disposable PostgreSQL service; migration; `SqlxPostgresExecutor`; subject, U+0000, concurrent terminal, and stale-generation scenarios; deterministic cleanup. | Complete | `production_postgres_contract` passed against local no-volume PostgreSQL 18 container `koduck-ai-pg-195-20260812`; the container was deleted immediately after the test. The full routed suite also passed with 77 tests under Rust 1.95. |
| T-3 | Make the three emitted check names required on `dev`. | A separately Accepted OCR for the exact GitHub repository-setting mutation, preflight snapshot, required check names, verification, and rollback. | Complete | Accepted `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` executed one approved protection request. At `2026-08-12T14:27:35+08:00`, `dev` required exactly the three strict GitHub Actions checks with App ID `15368`; unrelated protection fields remained disabled/absent, rulesets remained empty, and PR 1 was `CLEAN` / `MERGEABLE` with all checks successful. |

**Affected paths**: `Cargo.toml`; `.github/workflows/**`;
`koduck-ai/src/application/runner.rs`;
`koduck-ai/tests/postgres_subject_ownership.rs`;
`koduck-ai/src/adapters/history/postgres/**` only if the production-boundary
test requires a minimal public test seam; `docs/adr/INDEX.md`;
`docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`; and a subsequent
`docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` for the GitHub settings
operation.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: Add the workflow and integration test while the
pull request remains Draft. After all three checks exist and pass, an Accepted
OCR records the current `dev` protection state and adds exactly their names to
required status checks. If workflow verification or the external setting fails,
restore the captured protection state and keep the pull request Draft; removing
the workflow and integration test reverts repository source behavior without
touching runtime or production data.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering exception is proposed; new and modified maintained files
must remain within the standard guardrails or receive a decomposition review.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| CI-1 | `AGENTS.md` — Work Coordination, Review, And Delivery Sources Of Truth | Every Scope Routing verification command has a corresponding required green CI check for the latest pushed revision. | AC-1, AC-2, AC-3, AC-6 | Inspect workflow job/command mapping, capture the three check conclusions for one revision, and inspect `dev` required-check settings. |
| CI-2 | `AGENTS.md` — Scope Routing | Koduck AI changes run format, strict all-target/all-feature Clippy, and all-target/all-feature tests non-interactively. | AC-1, AC-2, AC-3 | Each named workflow job executes one exact routed command and reports exit code 0. |
| CI-3 | `docs/development/software-engineering-standard.md` — Testing And Change Design | External datastore risks are exercised through the production boundary or a behaviorally equivalent integration harness. | AC-4, AC-5 | The CI test job starts PostgreSQL and the named integration test runs migration and SQLx executor calls against it. |
| CI-4 | `AGENTS.md` — Core Rules | Verification produces structured diagnostics without exposing secrets or sensitive data. | AC-1, AC-5 | Workflow inspection finds no production secret input, and failing test output uses fixed disposable identifiers and owned errors. |
| CI-5 | `Cargo.toml` — workspace package metadata and committed `Cargo.lock` | The declared Rust version can compile the exact committed dependency graph. | AC-1, AC-2, AC-3 | Inspect `rust-version = "1.95"`, all three job pins, and successful locked-dependency command execution. |
| CI-6 | `koduck-ai/src/application/runner.rs` — `recover_append_failure` | Rust 1.95 compatibility must preserve recovery scheduling for `Unavailable` append failures, ignore an `Unavailable` scheduling failure, and propagate any other scheduling error. | AC-2, AC-3 | Add a focused recovery-error regression test, inspect that the let-chain retains all predicates, and run the durability and all-target/all-feature tests. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Two production SQLx terminal attempts race with an accepted interrupt and stale generation. | PostgreSQL integration harness and executor | Run the named concurrent integration scenario against the CI PostgreSQL service. | Exactly one terminal row commits; interrupt has the approved priority; the stale generation receives `Fenced`. | AC-4 | Pass | `production_postgres_contract` passed locally and in successful CI job `93988472996`. |
| Timeout and deadline | PostgreSQL service readiness or a verification job stalls. | GitHub Actions workflow | Inspect finite service health retries and per-job `timeout-minutes`; run the workflow. | Readiness failure or elapsed job timeout produces a non-success check rather than an indefinitely pending or successful check. | AC-1, AC-5 | Pass | Workflow has finite health retries and job timeouts; all jobs in run `31556012717` terminated successfully. |
| Cancellation and interruption | A superseded workflow run is cancelled while a newer revision is pending. | GitHub Actions concurrency policy and branch protection | Push a later revision or deterministically inspect the concurrency key and required-check settings. | A cancelled old run does not satisfy the newer revision; all three checks for the latest SHA must conclude success. | AC-1, AC-6 | Pass | Workflow cancellation is revision-grouped and `dev` protection reports `strict: true` with exactly the three GitHub Actions contexts, so a cancelled or stale revision cannot satisfy the latest-SHA gate. PR 1 exact head `f1154c59e73b52c2309485dfe45ab952517cd0c5` had all three successful checks. |
| Resource bounds and back-pressure | CI jobs or PostgreSQL state accumulate without bound. | GitHub Actions runner and disposable PostgreSQL service | Inspect one service per test job, bounded timeouts, no artifact upload, and no persistent volume; run the workflow. | Each job terminates within its timeout and retains no database volume or build artifact. | AC-1, AC-5 | Pass | Run `31556012717` terminated all jobs and stopped its PostgreSQL container; workflow defines no persistent volume or artifact upload. |
| Framework or trust-boundary rejection | Non-owned subject access or invalid persisted ownership is accepted by SQL or hidden by a unit double. | Production SQLx/PostgreSQL boundary | Run subject-ownership and stale-generation cases against PostgreSQL. | Non-owned access returns `NotFound`, stale generation returns `Fenced`, and neither changes durable items. | AC-4 | Pass | `production_postgres_contract` passed through `SqlxPostgresExecutor` locally and in CI job `93988472996`. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The workflow maps the latest `dev` pull-request revision to exactly three bounded Koduck AI verification jobs whose format job also gates the routed governance commands. | Workflow exists and a pull-request revision is pushed. | Inspect workflow triggers, concurrency, job names, service declarations, and timeouts; query the revision's check runs. | Trigger includes pull requests targeting `dev`; the latest SHA has exactly `koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres`; the format job executes both routed governance commands before its exact Rust format command; each job has a finite timeout; no job uploads an artifact. | Workflow diff and check-run JSON for the exact SHA. | Not Started | Run `31556012717` at SHA `18f1404a8edf6bb296960dadca8553e53e6b50e5` exposed exactly the three named finite jobs; each concluded `success`. |
| AC-2 | T-1 | The workspace and all three CI jobs use a compiler compatible with the committed dependency graph, and format/strict-Clippy succeed without changing recovery outcomes. | Committed lock and Rust sources are checked out in CI. | Run the focused recovery-error test before and after the rewrite; inspect root `rust-version`, every workflow toolchain pin, and the `recover_append_failure` predicates; then inspect logs for `cargo fmt --all --check` and `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`. | Root metadata and all three jobs specify Rust 1.95; the focused test proves non-`Unavailable` scheduling errors remain `TurnRunError::History`; the let-chain retains the `Unavailable`, valid `recovery_pending`, failed scheduling, and non-`Unavailable` propagation conditions; both commands exit 0 and their named checks conclude `success`. | Focused test RED/GREEN output, metadata/workflow/source diff, check-run conclusions, and command logs. | Pass | Mutation test failed when the scheduling error was discarded and passed with the let-chain; local Rust 1.95 format/Clippy passed, and run `31556012717` format plus Clippy jobs concluded `success`. |
| AC-3 | T-1 | The test check executes the complete routed test command successfully. | PostgreSQL service is healthy and the test database URL is supplied from disposable workflow values. | Run `cargo test -p koduck-ai --all-targets --all-features` in the test job. | Command exits 0; the test check concludes `success`; no test reports skipped PostgreSQL verification when the CI database variable is present. | Test log, test count, and check conclusion. | Pass | Local routed suite passed 77 tests; run `31556012717` completed the exact Cargo test step and `koduck-ai-test-postgres` concluded `success`. |
| AC-4 | T-2 | Production PostgreSQL behavior satisfies the selected datastore invariants. | Fresh CI PostgreSQL database; migration not previously applied. | Run the exact named PostgreSQL integration test through `SqlxPostgresExecutor`. | Migration succeeds; U+0000 round-trips; a different subject receives `NotFound`; concurrent terminal arbitration leaves one terminal with approved interrupt priority; stale generation receives `Fenced`; no rejected attempt adds an Item. | Named test output and SQL assertions from the CI run. | Pass | Named test passed locally against disposable PostgreSQL 18 and inside successful test job `93988472996` in run `31556012717`. |
| AC-5 | T-2 | The PostgreSQL test environment is disposable and bounded. | CI test job starts with no persisted service volume. | Inspect the PostgreSQL service health retries, job timeout, environment source, volume declarations, and artifact steps; execute the passing workflow. | The service has finite health retries, the job has a finite timeout, its database values are fixed disposable literals, no persistent volume or artifact upload exists, and the check concludes `success`. | Workflow definition and passing check result. | Pass | Workflow has bounded health retries and timeout, fixed disposable values, no volume/artifact; job `93988472996` initialized and stopped its container and concluded `success`. |
| AC-6 | T-3 | `dev` requires the exact three checks for the latest revision. | T-1 checks exist and pass; the settings OCR is Accepted. | Execute the OCR, then query the repository branch protection or ruleset API for `dev`. | The API requires exactly `koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres` for this task; a missing/failing check blocks merge; the prior protection snapshot and rollback command are recorded. | Accepted OCR, before/after settings JSON, and mergeability evidence. | Pass | `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` records the unprotected/empty-ruleset baseline, exact delete-protection rollback, and successful protection response. At `2026-08-12T14:27:35+08:00`, the API required exactly the three strict GitHub Actions checks and PR 1 was `CLEAN` / `MERGEABLE` with all three successful. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Not Started | The 2026-08-12 approval by `@linhai` (10:04:09+08:00, `Approve`) was invalidated on 2026-08-20 by the revision adopting the expanded format-check contract and is retained in the Change Log; reapproval is required. |
| A-2 | Complete task delivered | T-1 through T-3 are complete and AC-1 through AC-6 are Pass. | Implementation Plan, CI check runs, PostgreSQL test output, and Accepted OCR | Complete | T-1 through T-3 are complete; AC-1 through AC-6 pass. Workflow run `31568207691`, the production PostgreSQL integration result, and verified `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` provide the required evidence. |
| A-3 | Reciprocal ADD link synchronized, when applicable | Product-demand handoff does not apply. | Architecture Source metadata | N/A — this governance/CI task is not derived from product demand | Architecture Source records the specific N/A reason. |
| A-4 | Requirement levels satisfied | Every required and triggered field is complete or has a valid specific N/A reason for the current stage. | Structured document review | Complete | Structured review found no unresolved placeholders or missing `Verified`-stage fields; pushed-revision, T-3, acceptance-check, risk, and closure evidence are complete. |
| A-5 | Acceptance checks are decidable | Each check has one subtask, exact input, deterministic method, observable result, and evidence. | Structured acceptance-check review | Complete | AC-1 through AC-6 each name one subtask, an exact precondition, deterministic method, binary expected result, and evidence, including the Rust 1.95 compiler contract. |
| A-6 | Engineering exceptions governed, when applicable | No rule is exceeded, or a complete approved exception is present before implementation. | Engineering Exceptions and affected-file metrics | N/A — no exception applies | New workflow, integration test, and test additions remain below exception limits; the retained files above review thresholds keep their existing decomposition evidence. |
| A-7 | Contract and baseline risks covered, when applicable | CI-1 through CI-6 map to explicit checks and all five risk rows reach Pass before completion. | Traceability, Risk Coverage Matrix, and stable evidence | Complete | CI-1 through CI-6 map to AC-1 through AC-6; all acceptance checks and all five Risk Coverage Matrix rows are `Pass` with stable evidence. |
| A-8 | Governance validation passed | The deterministic governance validator and structured review both pass for the affected governance documents | Command output | Complete | `npm run validate --prefix tools/governance-validator` passes; all governance tests pass. |

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-0002-required-ai-ci-postgres-verification.md`
      under `docs/adr/`.
- [ ] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed code was deleted.
- [ ] If Decision Status is `Superseded`, set reciprocal replacement paths.
- [ ] If no record supersedes this one, retain `Superseded By: None`.
- [ ] Update this record's single row in `docs/adr/INDEX.md`.
- [ ] Confirm no active record or governed marker cites a stale path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-20 | Approval invalidated at 2026-08-20T00:54:39+08:00: the 2026-08-14 governance-command expansion of the required `koduck-ai-format` job was an approval-invalidating change to this record's accepted command contract that was never reapproved (automatic review `4974588517` on PR 3, P1). This revision formally adopts the expansion — Scope, Constraints, and AC-1 now define the format job as executing both routed governance commands before its exact Rust format command, keeping the exact three protected context names — and returns the record to `Proposed, Not Started` pending `@linhai`'s reapproval. Prior approval retained as history: Approver `@linhai`, Approval Time `2026-08-12T10:04:09+08:00`, Approval Evidence `Approve`, no Approval Context Revision. AC-1 returns to `Not Started` for post-reapproval execution; earlier evidence for the unchanged checks stays historical until re-executed after reapproval. | @zcode |
| 2026-08-14 | Addressed the required-governance-check review without mutating branch protection: the existing required `koduck-ai-format` job now runs both routed governance commands before its exact Rust format command, so either governance failure makes an already-required context fail while the exact three protected context names remain unchanged. A focused architecture contract test observed RED before the workflow mapping and GREEN after it; governance validation, format, strict Clippy, and the full Rust suite pass locally. | @codex |
| 2026-08-12 | Addressed automatic Review `4913685804`: required checks are repository-wide for `dev`, so the workflow must emit all three contexts for every pull request targeting `dev`, including changes outside the former path allowlist. Extended the existing architecture contract test, observed RED with the path filter, removed only that filter, and observed GREEN; format, strict Clippy, and all 91 tests passed locally. | @codex |
| 2026-08-12 | Executed Accepted `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md` after `@linhai` approval. The `dev` protection API now reports strict required checks for exactly `koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres`, all bound to GitHub Actions App ID `15368`; unrelated protection is absent/disabled and rulesets remain empty. PR 1 was `CLEAN` / `MERGEABLE` with all three checks successful and zero current unresolved threads. Marked T-3 complete, AC-6 and the cancellation risk row pass, completion items complete, and ADR implementation `Verified`. | @codex |
| 2026-08-12 | The repository owner made `hailingu/koduck` public. Recheck at `2026-08-12T14:17:52+08:00` confirmed repository visibility `public`, `dev` protection now accessible with the truthful baseline `404 Branch not protected`, and no repository rulesets. Satisfied the recorded exit criterion, restored Implementation Status from `Blocked` to its prior `In Progress`, moved T-3 to `In Progress`, and proposed the OCR later archived at `docs/adr/ocr/archive/OCR-0002-dev-required-ai-checks.md`; no GitHub protection setting was mutated. | @codex |
| 2026-08-12 | Workflow run `31556012717` passed all three jobs for exact SHA `18f1404a8edf6bb296960dadca8553e53e6b50e5`. Rechecked branch protection and rulesets at `2026-08-12T10:14+08:00`; both returned HTTP 403 requiring GitHub Pro or public visibility. Marked T-1 and T-2 complete, AC-1 through AC-5 pass, AC-6 fail, and the ADR `Accepted / Blocked`. Added ADR evidence paths to the workflow trigger so this status revision receives the same three checks. | @codex |
| 2026-08-12 | Completed local Rust 1.95 implementation verification: compiler-pin test RED-to-Green, recovery-error mutation RED then GREEN with the semantics-preserving let-chain, format and strict Clippy pass, 77 all-target/all-feature tests pass, and `production_postgres_contract` pass against disposable no-volume PostgreSQL 18 followed by container deletion. T-1 awaits pushed-revision checks; T-3 remains pending. | @codex |
| 2026-08-12 | Recorded `@linhai`'s exact `Approve` for the Rust 1.95 source-compatibility proposal at `2026-08-12T10:04:09+08:00`, set the ADR to `Accepted / In Progress`, and began T-1 verification. | @codex |
| 2026-08-12 | Approval-invalidating revision at `2026-08-12T10:00:47+08:00` added the minimum `runner.rs` compatibility rewrite after Rust 1.95 strict Clippy reported two `collapsible_if` failures. Preserved prior approval historically: Approver `@linhai`, Approval Time `2026-08-12T09:57:23+08:00`, Approval Evidence `Approve`, no Approval Context Revision. Reset Decision Status to `Proposed`, Implementation Status to `Not Started`, and T-1/A-1 to post-reapproval execution. | @codex |
| 2026-08-12 | Recorded `@linhai`'s exact `Approve` for the Rust 1.95 proposal at `2026-08-12T09:57:23+08:00`, set the ADR to `Accepted / In Progress`, and began T-1 with a RED compiler-contract test. | @codex |
| 2026-08-12 | Updated the Proposed compiler pin from Rust 1.94 to Rust 1.95 after `@linhai` explicitly selected Rust 1.95 for workspace metadata and every CI job; the committed dependency minimum remains Rust 1.94. | @codex |
| 2026-08-12 | Proposed one CI verification boundary with three routed required checks, disposable PostgreSQL production-boundary evidence, and a separately governed branch-protection operation after @linhai confirmed the approach in the active task. | @codex |
| 2026-08-12 | Recorded @linhai's exact `Approve`, set the ADR to `Accepted / In Progress`, and began T-2 with the required RED production-boundary test. | @codex |
| 2026-08-12 | Completed T-1 and T-2 locally: both architecture tests followed RED-to-Green, all three routed commands passed with 69 tests, and `production_postgres_contract` additionally passed against disposable PostgreSQL 18 before its no-volume container was deleted. T-3 and pushed-revision CI evidence remain pending. | @codex |
| 2026-08-12 | Set Implementation Status from `In Progress` to `Blocked` after GitHub returned HTTP 403 for both `dev` branch protection and repository rulesets, with the explicit requirement to upgrade the private repository to GitHub Pro or make it public. T-1 and T-2 remain complete; T-3 cannot proceed until the recorded exit criterion is met. | @codex |
| 2026-08-12 | Approval-invalidating revision at `2026-08-12T09:47:27+08:00` added the root workspace compiler contract and aligned every CI job to Rust 1.94 after runs `31553057678` and `31554660991` proved Rust 1.85 incompatible with committed `sqlx 0.9.0`. Preserved prior approval historically: Approver `@linhai`, Approval Time `2026-08-12T01:24:45+08:00`, Approval Evidence `Approve`, no Approval Context Revision. Reset Decision Status to `Proposed`, Implementation Status to `Not Started`, and all implementation/risk evidence to post-reapproval execution. | @codex |
