# OCR-0002: Green Domain Lifecycle Test

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Blocked
- **Date**: 2026-08-11
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-11T11:29:25+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: In Progress
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: Exit `101`; Rust error `E0015` at `koduck-ai/src/domain/mod.rs:75:12` because the `const fn` transition called non-const derived `PartialEq`; the test body did not run
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: @codex
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: No further attempt is permitted under this one-command OCR. A new source commit must remove the invalid const context, and a separately Accepted OCR must bind and test that commit.
- **Operation Type**: Build
- **Target Scope / Operation Owner**: Local development workspace; isolated target directory `/private/tmp/koduck-ai-green-e838128` / @codex
- **Input Source or Version**: Git commit `e8381284a58d7824aec0785ec819e04347153fdb`
- **Expected Output or Target State**: The exact domain-lifecycle test compiles and passes once; its disposable test executable and intermediate output are then deleted without promotion.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — no container operation
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — no Kubernetes operation
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: None — compilation stopped with `E0015` before producing the test executable; partial output was deleted from `/private/tmp/koduck-ai-green-e838128` at `2026-08-11T11:31:30+08:00`, and nothing was promoted
- **Dependencies**: Accepted, In Progress `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; archived Verified `docs/adr/ocr/archive/OCR-0001-red-domain-lifecycle-test.md`; Rust/Cargo toolchain and locked dependency set from the input commit
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

**Complete task outcome**: Execute exactly one isolated build and run of
`completed_turn_is_terminal` at commit
`e8381284a58d7824aec0785ec819e04347153fdb`, verify the test passes, capture
the result, and delete all disposable build output without promotion.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Build and run the focused lifecycle test once. | Commit `e8381284a58d7824aec0785ec819e04347153fdb`; package `koduck-ai`; target `domain_lifecycle`; exact test `completed_turn_is_terminal`; isolated target directory. | Command exits `0`; exactly one named test runs and passes; zero tests fail. | Timestamped command, exit code, and Cargo test summary. | Blocked | The exact command ran once and exited `101`; compiler error `E0015` reported an invalid non-const equality call inside `Turn::transition`; the test body did not run. No retry is permitted under this OCR. |
| T-2 | Dispose of the test executable and intermediate output and prove no promotion. | `/private/tmp/koduck-ai-green-e838128` and repository working tree. | The isolated target directory is absent after disposal, no release/container/deployment action occurred, and declared input source remains unchanged. | Path-existence check, Git diff inspection, and no-promotion statement. | Complete | At `2026-08-11T11:31:30+08:00`, the isolated directory was deleted and confirmed absent; declared source had zero diff from `e838128`; no release, image, deployment, or promotion occurred. |

## Eligibility [Required]

- [x] Uses the Accepted architecture and dependency boundary in `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`.
- [x] Is reversible; recovery is deletion of the isolated build output plus no-promotion evidence.
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

**Planned action and criterion**: Verify ADR-0001 is `Accepted`, `In Progress`;
verify `HEAD` equals the declared input commit; verify no diff exists under
`Cargo.toml`, `Cargo.lock`, or `koduck-ai/**`; verify the isolated target path
is absent; verify OCR-0001 is archived and `Verified`; and record Rust/Cargo
versions. Stop before Execute if any check differs.

**Actual result and stable evidence**: Pass at `2026-08-11T11:30:44+08:00`.
`HEAD` was exactly `e8381284a58d7824aec0785ec819e04347153fdb`;
the declared Cargo and `koduck-ai/**` paths had no diff; the isolated target
path was absent; OCR-0001 was archived and `Verified`; ADR-0001 was `Accepted`,
`In Progress`; the toolchain was `rustc 1.95.0 (59807616e 2026-04-14)` and
`cargo 1.95.0 (f2d3ce0bd 2026-03-21)`. Only OCR-0002 and its index row differed.

### Execute [Required]

**Planned action**: From the repository root, run exactly:

```sh
CARGO_TARGET_DIR=/private/tmp/koduck-ai-green-e838128 cargo test --locked -p koduck-ai --test domain_lifecycle completed_turn_is_terminal -- --exact
```

Capture the full command output and exit code. Do not run another test or
compile command under this OCR.

**Actual result and stable evidence**: Executed once after approval. Exit code
was `101`. The captured compiler output ended with:

```text
error[E0015]: cannot call non-const operator in constant functions
  --> koduck-ai/src/domain/mod.rs:75:12
error: could not compile `koduck-ai` (lib) due to 1 previous error
```

### Verify [Required]

**Success criterion**: Exit code is `0`; Cargo reports exactly one passed test
named `completed_turn_is_terminal`, zero failed tests, and zero ignored tests;
no release, image, deployment, or promotion is created.

**Actual result and stable evidence**: Fail. Exit code was `101`, no test ran,
and the exact success criterion was not met. The failure was contained to the
isolated build path and produced no promoted artifact.

### Stop and Recovery [Required]

**Stop condition**: Stop after the first command regardless of result. Also
stop without retry if preflight differs, a credential is requested, source or
configuration changes, or the command targets a path other than the declared
isolated directory.

**Recovery action**: Delete only
`/private/tmp/koduck-ai-green-e838128`; do not alter source, Cargo lock, Git
history, any container, or any environment.

**Recovery verification**: Confirm the isolated directory is absent, confirm
the input source paths have no diff from the declared commit, and record that
no artifact was promoted.

**Actual result and stable evidence**: Recovery completed at
`2026-08-11T11:31:30+08:00`. The isolated directory was deleted and confirmed
absent; declared source remained identical to commit
`e8381284a58d7824aec0785ec819e04347153fdb`; no artifact was promoted.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

- **Window and impact**: N/A — isolated local test build with no user,
  downstream, SLO, production, phased, or change-window impact
- **Observation**: N/A — no triggered operational observation window
- **Communication**: N/A — no triggered external communication requirement

## Closure [Required]

- **Final result**: Stopped after the authorized command failed; recovered and not promoted
- **Authorization review**: Pass — OCR-0002 was `Accepted` by `@linhai` at `2026-08-11T11:29:25+08:00`, before Execute
- **Subtask and evidence review**: Fail — T-1 is `Blocked` because the required passing test result was not achieved; T-2 recovery is complete
- **Requirement-level review**: Pass — every required and triggered field for the final `Blocked` attempt and archival is complete

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

Implementation Status is final `Blocked` with no further attempt permitted under
this OCR, so archival is triggered and completed in the same change:

- [x] Moved this file to `docs/adr/ocr/archive/OCR-0002-green-domain-lifecycle-test.md`.
- [x] No governed marker cites the pre-archive OCR path.
- [x] N/A — Decision Status is not `Superseded`; no reciprocal replacement applies.
- [x] Retained `Superseded By: None` because no record supersedes this OCR.
- [x] Updated the central index with the archived path and final `Blocked` status.
- [x] Confirmed no live record or governed marker cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-11 | Created the Proposed build OCR for one isolated green domain-lifecycle test at immutable source commit `e8381284a58d7824aec0785ec819e04347153fdb`. | @codex |
| 2026-08-11 | Accepted after `@linhai` identified OCR-0002 and supplied exact `Approve`; recorded Approval Time `2026-08-11T11:29:25+08:00` and entered `In Progress`. | @linhai |
| 2026-08-11 | Executed the approved command exactly once; observed exit `101` and Rust `E0015`, stopped without retry, deleted the isolated target directory, verified no source drift or promotion, recorded final `Blocked`, and archived the OCR. | @codex |
