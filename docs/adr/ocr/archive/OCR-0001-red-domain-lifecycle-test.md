# OCR-0001: Red Domain Lifecycle Test

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Verified
- **Date**: 2026-08-11
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-11T11:19:49+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Operation Type**: Build
- **Target Scope / Operation Owner**: Local development workspace; isolated target directory `/private/tmp/koduck-ai-red-7585f92` / @codex
- **Input Source or Version**: Git commit `7585f920d4a3b13cfe27475b8eee6212aef47b38`
- **Expected Output or Target State**: One intentional red TDD result: the exact domain-lifecycle test command exits non-zero because the `koduck_ai` production crate is absent, with no test executable or distributable artifact promoted.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — no container operation
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — no Kubernetes operation
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: None — the compiler stopped with `E0433` before producing the test executable; partial output under `/private/tmp/koduck-ai-red-7585f92` was deleted at `2026-08-11T11:21:18+08:00`, and nothing was promoted
- **Dependencies**: Accepted `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; Rust 1.85-or-newer toolchain; Cargo dependency cache or crates.io access for the locked source set
- **Related [Optional]**: [Koduck Trello card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`
- **Supersedes [Conditionally Required — this OCR replaces another]**: None
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

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

Planned content is required before `Accepted`; actual-result fields remain
`Pending` until their corresponding operation stage runs and are required
before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Execute exactly one isolated diagnostic build of the
TDD domain-lifecycle test at commit
`7585f920d4a3b13cfe27475b8eee6212aef47b38`, confirm it fails for the expected
missing production-crate reason, preserve the failure output as evidence, and
discard all partial target artifacts without promotion.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Produce and capture the intentional red lifecycle-test result. | Commit `7585f920d4a3b13cfe27475b8eee6212aef47b38`; package `koduck-ai`; test target `domain_lifecycle`; exact test `completed_turn_is_terminal`; isolated target directory. | Command exits non-zero and reports that `koduck_ai` cannot be resolved because no production library target exists; no test binary runs. | Timestamped command, exit code, and compiler diagnostic. | Complete | The exact command ran once after approval and exited `101`; compiler error `E0433` at `koduck-ai/tests/domain_lifecycle.rs:3:5` reported unresolved crate/module `koduck_ai`; compilation stopped and the test body did not run. |
| T-2 | Dispose of partial build output and prove no promotion. | `/private/tmp/koduck-ai-red-7585f92` and repository working tree. | The isolated target directory is absent after disposal, no release/container/deployment action occurred, and tracked source remains unchanged from the input commit apart from this OCR and index row. | Path-existence check, Git diff inspection, and no-promotion statement. | Complete | At `2026-08-11T11:21:18+08:00`, the explicit target directory was deleted and confirmed absent; `Cargo.toml`, `Cargo.lock`, and `koduck-ai/**` had zero diff from commit `7585f92`; no image, release, deployment, or promotion occurred. |

## Eligibility [Required]

- [x] Uses the Accepted architecture and dependency boundary in `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`.
- [x] Is reversible; recovery is deletion of the isolated partial target output plus no-promotion evidence.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format,
      signing, credentials, deployment topology, API/schema/protocol,
      authentication, security policy, data lifecycle, dependency, provider,
      or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] N/A — no automatic-review mechanism configured; repository inspection
      found no automatic-review configuration. This absence is not treated as
      approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: Verify ADR-0001 is `Accepted` and `In
Progress`; verify `HEAD` equals the declared input commit; verify no source or
manifest diff exists under `Cargo.toml`, `Cargo.lock`, or `koduck-ai/**`;
verify the isolated target path is absent or empty; and record `rustc` and
`cargo` versions. Stop before Execute if any check differs.

**Actual result and stable evidence**: Pass at `2026-08-11T11:20:22+08:00`.
`HEAD` was exactly `7585f920d4a3b13cfe27475b8eee6212aef47b38`;
the declared Cargo and `koduck-ai/**` input paths had no diff from that commit;
the isolated target path was absent; ADR-0001 was `Accepted`, `In Progress`;
the toolchain was `rustc 1.95.0 (59807616e 2026-04-14)` and
`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`. The only worktree changes were this OCR
and its central index row.

### Execute [Required]

**Planned action**: From the repository root, run exactly:

```sh
CARGO_TARGET_DIR=/private/tmp/koduck-ai-red-7585f92 cargo test --locked -p koduck-ai --test domain_lifecycle completed_turn_is_terminal -- --exact
```

Capture the full command output and exit code. Do not run another test or
compile command under this OCR.

**Actual result and stable evidence**: Executed once after approval. Exit code
was `101`. The captured compiler output ended with:

```text
error[E0433]: cannot find module or crate `koduck_ai` in this scope
 --> koduck-ai/tests/domain_lifecycle.rs:3:5
error: could not compile `koduck-ai` (test "domain_lifecycle") due to 1 previous error
```

### Verify [Required]

**Success criterion**: The command exits non-zero; the compiler diagnostic
identifies unresolved crate or module `koduck_ai` at
`koduck-ai/tests/domain_lifecycle.rs`; the test body does not run; and no
distributable artifact, image, release, deployment, or promotion is created.

**Actual result and stable evidence**: Pass. The observed exit was non-zero
(`101`), the diagnostic was the exact unresolved `koduck_ai` production-crate
failure at the specified test import, no test executable ran, and no
distributable artifact or promotion was created.

### Stop and Recovery [Required]

**Stop condition**: Stop after the first command regardless of result. Also
stop without any retry if preflight differs, a credential is requested, a
source/configuration write is observed, or the command attempts a target other
than `/private/tmp/koduck-ai-red-7585f92`.

**Recovery action**: Delete only the explicit isolated directory
`/private/tmp/koduck-ai-red-7585f92`; do not alter source, the Cargo lock, Git
history, any container, or any environment.

**Recovery verification**: Confirm the isolated directory is absent, confirm
the input source paths have no diff from the declared commit, and record that
no artifact was promoted.

**Actual result and stable evidence**: Recovery completed at
`2026-08-11T11:21:18+08:00`. The explicit isolated directory was deleted and
confirmed absent; the declared input paths remained identical to commit
`7585f920d4a3b13cfe27475b8eee6212aef47b38`; no artifact was promoted.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

- **Window and impact**: N/A — isolated local diagnostic build with no user,
  downstream, SLO, production, phased, or change-window impact
- **Observation**: N/A — no triggered operational observation window
- **Communication**: N/A — no triggered external communication requirement

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review,
and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks `Complete` and `Verified`; stop and recover, roll back, or use a
truthful retirement path as applicable. `N/A` is valid only when the review's
stated condition does not apply.

- **Final result**: Completed as the expected intentional red result; partial
  output recovered and not promoted
- **Authorization review**: Pass — OCR-0001 was `Accepted` by `@linhai` at
  `2026-08-11T11:19:49+08:00`, before preflight and Execute
- **Subtask and evidence review**: Pass — both subtasks are `Complete` with
  exit-, diagnostic-, path-, source-, disposition-, and no-promotion evidence
- **Requirement-level review**: Pass — all required and triggered content for
  `Verified` is complete; inactive conditional content carries explicit N/A
  reasons

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

Implementation Status is `Verified`, so archival is triggered and completed in
the same change:

- [x] Moved this file to `docs/adr/ocr/archive/OCR-0001-red-domain-lifecycle-test.md`.
- [x] No governed marker cites the pre-archive OCR path.
- [x] N/A — Decision Status is not `Superseded`; no reciprocal replacement applies.
- [x] Retained `Superseded By: None` because no record supersedes this OCR.
- [x] Updated this record's single row in `docs/adr/INDEX.md` with the archived path and `Verified` status.
- [x] Confirmed no live record or governed marker cites the pre-archive path; the central index is the archival reference.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-11 | Created the Proposed build OCR for one isolated intentional red domain-lifecycle test at immutable source commit `7585f920d4a3b13cfe27475b8eee6212aef47b38`. | @codex |
| 2026-08-11 | Accepted after `@linhai` identified OCR-0001 and supplied exact `Approve`; recorded Approval Time `2026-08-11T11:19:49+08:00` and entered `In Progress` for preflight and execution. | @linhai |
| 2026-08-11 | Executed the approved command exactly once, observed the expected exit `101` and `E0433` unresolved `koduck_ai` red result, deleted the isolated target directory, verified no source drift or promotion, set Implementation Status to `Verified`, and archived the OCR. | @codex |
