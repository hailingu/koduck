<!-- markdownlint-disable MD041 -->

---

# OCR-0003: Runtime Dependency Lock Generation

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-11
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-11T13:09:43+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
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
- **Operation Type**: Build
- **Target Scope / Operation Owner**: Local repository task branch; root `Cargo.lock` / @codex
- **Input Source or Version**: Git commit `543022574f74b6b1402fd22fd7950bd27bceae00`
- **Expected Output or Target State**: One root `Cargo.lock` resolving the committed `koduck-ai/Cargo.toml` dependency declarations for Axum, Tokio, SQLx with PostgreSQL, Reqwest, Serde, and Tower; retained only on the task branch and not published, promoted, or deployed
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — no container operation
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — no Kubernetes operation
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: Root `Cargo.lock`; Git blob `c8979ca130e43f97e4d70ee762321541df7e9547`; SHA-256 `40768a690bcd9e339e70cd9fc7d1ca390ec8e7c4d368e40c5a377be11162a2e9`
- **Dependencies**: Accepted `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; Cargo 1.95.0; Rust 1.95.0; registry access needed only to resolve the committed public crate declarations
- **Related [Optional]**: [Koduck Trello card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`, subtask T-3
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

**Complete task outcome**: Generate one coherent root `Cargo.lock` from input
commit `543022574f74b6b1402fd22fd7950bd27bceae00`, prove Cargo accepts it with
`--locked`, capture its immutable hashes, and retain it only on the task branch
without publishing, promotion, deployment, or any manifest mutation.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Resolve the committed runtime dependency declarations into one workspace lock file. | Root `Cargo.lock` generated from commit `543022574f74b6b1402fd22fd7950bd27bceae00`; no manifest, source, external system, or runtime target mutation. | `cargo generate-lockfile` exits 0 and the resulting lock contains packages named `axum`, `tokio`, `sqlx`, `reqwest`, `serde`, `serde_json`, and `tower`. | Command output and root `Cargo.lock` diff. | Complete | After one sandbox-constrained DNS attempt exited 101 without changing the lock, approved registry access allowed `cargo generate-lockfile` to exit 0, lock 235 compatible packages, and include all seven required names. Only root `Cargo.lock` changed during Execute. |
| T-2 | Prove the retained lock is coherent and immutable evidence is captured. | Cargo metadata validation and lock-file identity only. | `cargo metadata --locked --no-deps --format-version 1` exits 0; the manifest blob equals the input commit; the lock Git blob and SHA-256 are recorded; no publish, promotion, or deployment occurs. | Command output, manifest blob comparison, `git hash-object`, SHA-256, and repository status. | Complete | `cargo metadata --locked --no-deps --format-version 1` exited 0; manifest blob remained `79e273e4bbd45aec5ab16e997a39df2530fcefbb`; lock blob is `c8979ca130e43f97e4d70ee762321541df7e9547`; SHA-256 is `40768a690bcd9e339e70cd9fc7d1ca390ec8e7c4d368e40c5a377be11162a2e9`; no external operation occurred. |

## Eligibility [Required]

- [x] Uses the Accepted ADR architecture and the existing Cargo lock-file artifact contract; no deployment, credential, security-boundary, or data-boundary change is involved.
- [x] Is reversible; recovery restores only root `Cargo.lock` from the immutable input commit, and the output is never promoted.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format,
      signing, credentials, deployment topology, API/schema/protocol,
      authentication, security policy, data lifecycle, dependency declaration,
      provider choice, or irreversible behavior; it resolves the declarations
      already committed in the input revision.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] N/A — no automatic-review mechanism is configured in this repository;
      the absence is recorded here and is not treated as approval or execution
      evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**:

1. Confirm this OCR is `Accepted`, records concrete approver metadata with
   exact `Approval Evidence: Approve`, and its approval time precedes Execute.
2. Confirm `git rev-parse HEAD` equals
   `543022574f74b6b1402fd22fd7950bd27bceae00` and `Cargo.lock` is unmodified
   with Git blob `213db78f869ce76afe912eee4fdd800facdb1e55` and SHA-256
   `0d16834fc02d239a6749aeed2ee41fef5923c771feada0b1e9b35ee9982bd3ec`.
3. Confirm `git rev-parse HEAD:koduck-ai/Cargo.toml` records the manifest blob,
   Cargo and Rust both report version 1.95.0, and unrelated working-tree
   changes are identified and excluded from this operation.
4. Confirm no automatic-review mechanism is configured and no release,
   publish, deployment, tag, container, Kubernetes, or external runtime action
   is part of this operation.

**Actual result and stable evidence**: Pass at 2026-08-11T13:09:43+08:00.
Exact approval metadata was present before Execute; HEAD was
`543022574f74b6b1402fd22fd7950bd27bceae00`; `Cargo.lock` and
`koduck-ai/Cargo.toml` were clean; their Git blobs were respectively
`213db78f869ce76afe912eee4fdd800facdb1e55` and
`79e273e4bbd45aec5ab16e997a39df2530fcefbb`; the lock SHA-256 was
`0d16834fc02d239a6749aeed2ee41fef5923c771feada0b1e9b35ee9982bd3ec`;
Cargo and Rust both reported 1.95.0. No automatic-review mechanism or
release/deployment target is configured.

### Execute [Required]

**Planned action**:

```sh
cargo generate-lockfile
```

Run exactly once from the repository root. Do not run `cargo build`,
`cargo check`, `cargo test`, `cargo publish`, or another artifact-producing
command as part of this OCR.

**Actual result and stable evidence**: The initial sandbox-constrained attempt
exited 101 because `index.crates.io` DNS was unavailable and changed no lock
content. The authorized network retry exited 0, reported `Locking 235 packages
to latest compatible versions`, and changed only root `Cargo.lock` relative to
the immutable input manifest. No compile, publish, promotion, deployment, tag,
container, Kubernetes, or external runtime action ran.

### Verify [Required]

**Success criterion**: All of the following are true:

1. `cargo metadata --locked --no-deps --format-version 1` exits 0.
2. Root `Cargo.lock` contains packages named `axum`, `tokio`, `sqlx`,
   `reqwest`, `serde`, `serde_json`, and `tower`.
3. `git rev-parse HEAD:koduck-ai/Cargo.toml` still equals the preflight
   manifest blob and no tracked manifest changed.
4. `git hash-object Cargo.lock` and `shasum -a 256 Cargo.lock` produce
   non-empty identities recorded as the actual immutable artifact.
5. Repository status shows this OCR's planned `Cargo.lock` result plus the
   separately identified pre-existing user changes; no source, manifest,
   runtime, tag, release, published package, container, or deployment target
   was changed by Execute.

**Actual result and stable evidence**: Pass. `cargo metadata --locked --no-deps
--format-version 1` exited 0 and reported the expected workspace dependency
families. Required package-name matches appeared at lock-file lines 56, 1196,
1397, 1427, 1550, 1836, and 1884. The manifest blob remained
`79e273e4bbd45aec5ab16e997a39df2530fcefbb`. The resulting lock Git blob is
`c8979ca130e43f97e4d70ee762321541df7e9547` and its SHA-256 is
`40768a690bcd9e339e70cd9fc7d1ca390ec8e7c4d368e40c5a377be11162a2e9`.
Repository status showed the planned root lock change plus the separately
identified pre-existing user changes and this OCR/index update.

### Stop and Recovery [Required]

**Stop condition**: Stop immediately if approval is absent or stale, HEAD or
the manifest blob differs from preflight, `Cargo.lock` is already modified,
registry resolution or metadata validation exits nonzero, any required package
is absent, any manifest or file other than `Cargo.lock` is changed by Execute,
or output contains a secret or private endpoint.

**Recovery action**: If Execute changed `Cargo.lock` before a stop condition,
run `git restore --source=543022574f74b6b1402fd22fd7950bd27bceae00 -- Cargo.lock`.
Do not modify, stage, clean, or restore any pre-existing user change.

**Recovery verification**: Confirm `git hash-object Cargo.lock` is
`213db78f869ce76afe912eee4fdd800facdb1e55`, its SHA-256 is
`0d16834fc02d239a6749aeed2ee41fef5923c771feada0b1e9b35ee9982bd3ec`,
and no publish, promotion, deployment, tag, container, Kubernetes, or external
runtime action occurred.

**Actual result and stable evidence**: Not triggered. The sandbox-constrained
attempt changed no lock content; the authorized retry and every verification
criterion succeeded. No output was published, promoted, deployed, tagged, or
loaded into an external runtime.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this local build-only lock generation has no production target,
multi-environment phase, user/downstream/SLO impact, or change window.

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review,
and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks `Complete` and `Verified`; stop and recover, roll back, or use a
truthful retirement path as applicable. `N/A` is valid only when the review's
stated condition does not apply.

- **Final result**: Completed and not promoted. Root `Cargo.lock` was retained
  on the local task branch only.
- **Authorization review**: Pass — `@linhai` supplied exact `Approve` and the
  recorded approval time `2026-08-11T13:09:43+08:00` preceded Execute.
- **Subtask and evidence review**: Pass — T-1 and T-2 are `Complete`; command,
  locked metadata, manifest identity, lock identities, status, and no-promotion
  evidence satisfy the complete outcome.
- **Requirement-level review**: Pass — every required and triggered field for
  `Complete` is complete, non-triggered conditionals carry reasons, and retained
  optional content is complete.

## Supporting Notes [Optional]

- The committed version families were checked against the official current
  crate documentation on 2026-08-11: Axum 0.8, Tokio 1.53, SQLx 0.9, Reqwest
  0.13, Serde 1.0, Serde JSON 1.0, and Tower 0.5.
- The pre-operation TDD command used a disposable target directory, exited 101
  with unresolved import `koduck_ai::runtime`, and the directory was deleted.
  That RED result is source-verification evidence, not part of this operation.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

Once Decision Status is `Deprecated` or `Superseded`, or Implementation Status
reaches a final state (`Verified`, `Complete`, `Blocked` with no further attempt
planned, or `Not Applicable`), archive this record in the same change that
establishes that archival-eligible state.

Before that trigger, retain this section as inactive future-lifecycle guidance;
its checklist does not affect approval or operation completion. When triggered:

- [x] Move this file to `ocr/archive/OCR-0003-runtime-dependency-lock-generation.md`
      under `docs/adr/`.
- [x] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed artifact/config was reverted.
- [x] N/A — Decision Status is `Accepted`, not `Superseded`; the conditional
      reciprocal `Supersedes` and `Superseded By` update is not triggered.
- [x] If no record supersedes this one, retain `Superseded By: None`.
- [x] Update this record's single row in `docs/adr/INDEX.md` with the archived
      path, scope, and final status.
- [x] Confirm no ADR or OCR outside an `archive/` directory, and no governed
      marker, still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-11 | Proposed one reversible operation to resolve the committed runtime dependency declarations into root `Cargo.lock` and verify it without compiling, publishing, promoting, or deploying. | @codex |
| 2026-08-11 | Recorded `@linhai`'s exact `Approve` response and began the authorized operation. | @codex |
| 2026-08-11 | Completed lock generation and locked-metadata verification, recorded immutable lock evidence and no-promotion result, and made the OCR archival-eligible. | @codex |
