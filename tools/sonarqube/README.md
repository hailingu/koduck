<!-- ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md -->
# Local SonarQube push gate

The repository owner authorized the original workflow directly on 2026-09-05 in
task `01a06ecc-0585-7b63-8311-2022f1a42315`: enable local SonarQube analysis
without an ADR/OCR, and permit Git pushes only after incremental SonarQube
findings reach zero. The owner explicitly waived an ADR for implementing that
workflow; this paragraph records the original authorization as history. The
accepted `docs/adr/ADR-0017-push-boundary-sonarqube-verification.md` amended it
on 2026-09-24: mandatory Sonar analysis moved from every commit to the push
boundary, and the versioned `pre-commit` hook was removed. Routine
installation, disposable test databases and analysis through these entry
points still need no ADR, OCR or repeated approval. Other changes retain
their normal governance. Historical ADR evidence is not rewritten or
presented as approval of this change.

## Installation and use

Run `sh tools/sonarqube/install.sh` in each checkout. It sets the repository-local
`core.hooksPath=.githooks` and enables the versioned `pre-push` hook, without
installing coverage dependencies. It refuses to overwrite another hook setup,
including unmanaged hooks under the default hooks directory, because setting
`core.hooksPath` would silently disable them. Existing worktrees share Git config;
each worktree must contain this versioned hook directory.

Prerequisites are Python 3.10+, Rust 1.95 with `llvm-tools-preview`,
`cargo-llvm-cov 0.9.0`, SonarScanner CLI `7.3.0.5189`, and a reachable local
SonarQube supporting Rust, JavaScript and Python (verified on Community Build
`26.8.0.126808`, Rust analyzer `1.8.0.3284`).

The shared entry point `scripts/sonar-quality-gate.sh` follows the existing
PlotWeave gate structure. It uses `KODUCK_SONAR_TOKEN` from the calling shell,
or loads `~/.zshrc` through interactive zsh when that export is absent. It never
falls back to another project's generic `SONAR_TOKEN`. With no
`KODUCK_AI_TEST_DATABASE_URL`, it creates and cleans a disposable PostgreSQL 18
Docker container with generated credentials. You may supply an **isolated
disposable** database URL instead. Never use the application database. No token goes in arguments,
repository files, reports or output. Commands started by Git inherit its process
environment; GUI clients can use the zsh fallback for the token. Docker must be available
when the workflow creates its own fixture database.

- `python3 tools/sonarqube/gate.py pre-push`: consume Git's ref-update lines
  from stdin and check every proposed commit target, including peeled tags.
  Deletions introduce no source and require no analysis. This command never
  performs a push itself.
- `python3 tools/sonarqube/gate.py check --revision HEAD`: check a committed
  revision manually. Use `--base <ancestor-SHA>` to select an explicit baseline.
- The retired `pre-commit` mode is no longer an accepted mode. The direct
  Python CLI rejects it with exit status 2 through argument parsing before
  loading credentials, creating the database fixture, acquiring the scan lock
  or scanning. Calling the shell wrapper with the unsupported `pre-commit`
  argument may still load credentials before Python rejects it; changing that
  wrapper is outside the amendment's scope. Neither invocation is successful
  verification.

Creating a local commit performs no Sonar, coverage, Cargo, database or token
work. Commits are local checkpoints and MUST NOT be claimed as
Sonar-verified. Every push, including a documentation-only push, scans each
distinct proposed target afresh regardless of file types or previously
successful analysis; this avoids an accidental extension-based bypass when
scanner, dependency or build inputs change. Findings, a failed quality gate
or insufficient coverage block **push**, completion and review-ready status.
Do not use `--no-verify` to claim gate success. CI runs the separate checks
described below.

## Source identity and increment definition

The scanner uses a private temporary clone with real Git history and checks
out the exact proposed commit. Evidence binds the tree, baseline commit and
policy hash; it never labels an old revision as the new source. Pre-push
verifies the proposed object, not the caller's checkout HEAD, index or
unstaged work. Changing source, baseline or executable policy invalidates
evidence.

The baseline is `git merge-base dev <target>` from local history, without an
implicit fetch. An explicit `--base` must be an ancestor of the target. Each analysis
pair scans this baseline and the target with identical source scope, exclusions
and analyzer installation. The baseline scan is comparison evidence, not an
attempt to claim that historical code passes today's gate. The target is left
on the dashboard, including when it fails; failed target results are never
replaced by a recovery scan of old code.

Incremental issues are the positive multiset difference between unresolved
baseline and target Sonar issues, keyed by rule, component, source hash and
message. Multiplicity matters: an additional identical defect is still new.
Open, confirmed and accepted-but-unfixed issues are included. The existing token
cannot read security hotspots; hotspot review is not independently checked. No issue
is automatically accepted, suppressed, resolved or deleted. Unstable fingerprints
may conservatively require fixing a finding; they never waive a new finding.

The existing server's `PREVIOUS_VERSION` period is not guaranteed to represent a
Git feature diff. Therefore its `new_coverage` is not claimed as feature coverage.
The workflow imports same-snapshot Rust product coverage, intersects executable
report lines with `git diff --unified=0 <base> <target>`, and requires at least
**80%** coverage of those changed product executable lines. Root `tools/`,
`scripts/`, and `.githooks/` remain subject to static analysis and CI checks
but do not enter this coverage fraction. Zero changed executable lines is permitted;
a missing coverage report is an error. An absent file record is permitted only
when the project-level file metric confirms its `lines_to_cover` is zero, or
after successful Rust compilation a conservative grammar confirms the entire
file contains only module/import declarations. Executable or unfamiliar syntax
never receives this treatment. The server's
analysis-bound Quality Gate must independently be `OK`, including all of its
configured conditions. This is the explicit Git-based incremental definition
authorized for this local Community workflow; no server administration token
or New Code setting mutation is needed.

## Execution and failure contract

`config.json` pins the host, project, analyzer exclusions and time budgets.
Both scans analyze the repository with the same main/test classification.
`rust-coverage.sh` runs only `cand_11_correction_admission`,
`cand_12_projection`, and `postgres_cand_11` integration targets through
cargo-llvm-cov, with three test threads and one default build job. The
integration-target selection excludes inline `#[cfg(test)]` unit-test code
from LCOV; a future product change outside these targets must update the
selection or fail the unchanged 80% gate. The report is converted to Sonar
generic XML and imported through `sonar.coverageReportPaths`. Compiler output
and reports live in disposable checkouts and are removed after evidence is
captured. CI separately runs all Rust tests, governance tests, Python gate
tests, format, and Clippy checks. The repository owner `@linhai`'s direct
instruction on 2026-09-24, given in the ADR-0005 CAND-12 implementation task,
authorized adding the `cand_12_projection` target during that ADR's delivery
together with the affected-paths expansion the update requires; it changes no
threshold, scope rule, or admission predicate.

Each command has a timeout; scanner submission is bounded to 600 seconds and
compute settlement to 300 seconds. Cancelled subprocess groups are killed and
reaped. Private test/scanner output is not echoed. Failure prints an owned
diagnostic; run the named focused verification command separately to debug.
No automatic scan retry or automatic remediation loop runs inside a hook.

A host-local lock serializes scans in this environment. Following the owner's
explicit selection of PlotWeave-style project-level checks, the existing token
reads issues and file metrics without requiring `/api/ce/component` permissions.
The scanner's own compute task is awaited and its analysis-bound Quality Gate is
checked, but project-level issue reads cannot prove isolation from concurrent
scans on other hosts. Do not run other writers against this project during a gate.
Every push runs a fresh base/target pair; saved evidence never skips scanning.
Incomplete pages, missing metrics and API failures block admission. Evidence is
stored atomically under Git's common directory at `sonarqube/`, recording tree,
revision, baseline, policy, task/analysis IDs, issue counts and coverage fractions.

## Admission evidence from an actual push

After `require_pass` accepts a target's record, `gate.check_revision` prints and
flushes exactly one complete stdout line with these fields in this order:

`Sonar push admitted: <revision> analysis=<analysis-id> tree=<tree> base=<base> policy=<policy> new_issues=<n> quality_gate=<status> covered=<c> coverable=<d>`

`<revision>` is the resolved proposed commit and `<analysis-id>` its nonempty
analysis ID. The tree, base and policy fields are the identity passed to
`require_pass`; new_issues, quality_gate, covered and coverable are the
identically named record values it accepted. Capture the complete line from
the actual canonical pre-push invocation using the **combined stdout and
stderr of `git push`**: the gate itself writes stdout, but Git routes hook
stdout to its own stderr, so the line appears in the push output. The
revision, tree, base and policy fields MUST equal the current values
recomputed by the read-only procedure below. A missing field, the former
shorter line, absent admission output or any mismatch does not qualify. Do
not read persisted evidence or compute an evidence filename to qualify a
line; failed results may still be stored before `require_pass` rejects them.

Admission is per target. An earlier target's line remains valid when a later
target fails or the remote rejects the Git push, provided that target's
identity still matches. Other targets without their own qualifying lines
remain unverified; a deletion-only invocation supplies no line; and neither
the overall hook or Git exit status nor persisted files prove target
admission. Matching successful pre-push evidence MAY satisfy the Sonar
portion of local completion and review-ready verification instead of a
second manual `check`; record the actual invocation, the admission line and
the identity-comparison result, and report overall push success or failure
separately. Every later push still scans afresh, and the admission line is
not proof of remote Git publication.

### Recomputing the current identity (read-only)

Run this from the repository root with the workflow's required Python 3.10+
(the `python3` on PATH), replacing the `HEAD` argument with the admitted
target's exact commit. It uses existing helpers, reads no credentials or
stored evidence, and starts no scanner or database. Compare its four output
fields with the admission line; the analysis ID is retained from the captured
post-`require_pass` output.

```sh
PYTHONDONTWRITEBYTECODE=1 python3 - HEAD <<'PY'
import json
import sys
from pathlib import Path

root = Path.cwd()
sys.path.insert(0, str(root / "tools/sonarqube"))
from gate import policy_id
from git_snapshot import feature_base, git

revision = git(root, "rev-parse", sys.argv[1] + "^{commit}")
print(json.dumps({
    "revision": revision,
    "tree": git(root, "rev-parse", revision + "^{tree}"),
    "base": feature_base(root, revision),
    "policy": policy_id(),
}, sort_keys=True))
PY
```

## Local scanning and CI

The owner removed the Docker runner build workflow on 2026-09-06. The local
pre-push hook invokes the installed scanner directly. No custom
runner image, registration or readiness variable is required. The optional
PostgreSQL test fixture uses the existing upstream image without building it.

GitHub CI retains formatting, Clippy, PostgreSQL tests, governance validation,
and hook regression checks. It does not submit SonarQube analyses or depend on
a local runner. Sonar admission is enforced by local hooks; CI cannot establish
Sonar compliance if someone bypasses those hooks.

## Verification

### Rust test-module coverage boundary — 2026-09-23

PR 15 reviews `5286757657`, `5287659484`, and `5288161841` identified three
unsafe ways to classify Rust by path or raw text. The gate now uses the existing
strict Clippy build's dependency files: a Rust file is test-only only when it
appears in a test build and in no non-test build. This single classification
governs both changed lines and imported coverage.

| State / precondition | Action / entry point | Observable outcome and invariant | Verification |
| --- | --- | --- | --- |
| A Rust file appears only in test-build dependencies | Classify the Git diff and imported coverage | Its lines never increase production covered or coverable counts | `test_rust_test_modules_do_not_count_as_changed_production` and compiler-scope fixture |
| A changed Rust production sibling and a test-only module coexist | Classify the same revision | Production changes remain counted; test-only changes cannot mask an under-80% production diff | The same focused Git fixture and the normal feature gate |
| A comment contains a fake `#[cfg(test)] mod foo;` but the real `mod foo;` is active | Classify the Git diff and imported coverage | The compiled `foo.rs` remains production in both paths | `test_comment_cannot_exempt_compiled_rust_module_from_coverage` |
| A production `#[path]` declaration imports `tests/engine.rs` | Classify the Git diff and coverage, then check Sonar's test-path mapping | The file is included in both local sets; the gate rejects a changed file that Sonar would classify as a test | `test_compiled_rust_module_under_tests_path_remains_production` and scan-scope check |
| Clippy's current compilation evidence is missing or malformed | Classify Rust scope | Stop verification instead of granting any test-only exemption | Compiler-scope parser regression |

This serial, local classification has no retry or concurrent transition. A
new source revision is classified from its own snapshot. Rust files absent
from the proven test-only set remain production, regardless of comments,
attributes, names, or directories. A changed production file under Sonar's
test-path patterns blocks the gate because Sonar cannot analyze it as main
source with the current static scanner mapping.

### Correction retry and ancestry snapshot — 2026-09-23

PR 15 reviews `5286757657` and `5286672140` exercise CA-03/CA-04 against a
concurrent maintenance writer. The PostgreSQL correction adapter owns these
invariants at direct admission and reconciliation entry points.

| State and event ordering | Expected outcome | Invariant | Verification |
| --- | --- | --- | --- |
| Exact retry identity remains unchanged through payload read | Return the stored correction | Metadata and body describe one identity | Existing exact-retry tests |
| Writer retargets the matching row after a metadata precheck | Reject `IdentityConflict` | A body from a different identity never authenticates the old metadata | New two-connection race test |
| Writer retargets an ancestor after summary and before streamed read | Reject unsupported root or corruption | The streamed ancestry independently satisfies CA-03 | New two-connection race test |
| Writer grows a payload between statements | Reject `ResourceLimit` before transferring the body | Every body projection enforces the cap in its own snapshot | Existing payload-read race tests |
| A streamed chain has multiple invalid properties | Validate its summary after the final row | The existing CA-03/CA-06 error precedence remains stable after splitting the validator | Existing ancestor corruption, limit, and race tests |

The identity and ancestry race fixtures failed against the prior implementation
with an incorrectly successful `Item`, then passed after the second-snapshot
checks. The full Rust targets, Clippy, format check, 45 Sonar tool tests, 184
governance tests, and governance validation passed with three test threads and
one Cargo build job for the Rust run.

### Product Rust coverage — 2026-09-23

The owner selected production Rust changed-line coverage at the existing 80%
threshold, generated from named integration tests, with root tooling excluded.
The current PR review identified inline `#[cfg(test)]` code as a false source of
coverage credit. The gate still uses compiler dependency evidence to distinguish
dedicated test modules from production modules compiled under a `tests/` path.

| State / precondition | Action / entry point | Observable outcome and invariant | Verification |
| --- | --- | --- | --- |
| Production lines and inline `#[cfg(test)]` lines share one Rust file | Generate LCOV through named integration targets | Only production-compiled lines can raise the product fraction | Rust coverage script test and disposable cargo-llvm-cov fixture |
| Root tooling changes beside product Rust | Intersect Git diff with imported coverage | Tooling never raises or lowers the product numerator or denominator; static analysis still includes it | Product-scope and scanner-property tests |
| A changed production module lives under a directory named `tests` | Classify compiler dependencies and Sonar mapping | The gate includes its lines locally and stops when Sonar would classify the file as a test | Compiler-scope and scan-scope regressions |
| An executable production file has no LCOV record | Calculate changed-line coverage | Admission fails instead of treating missing coverage as zero coverable lines | Missing-report regression |
| Test or scanner process fails | Run the local gate | No passing evidence is recorded for the revision | Script failure and gate tests |

Coverage generation is serial at the gate boundary. Each invocation uses a
source-bound disposable checkout and report, so a later push cannot reuse an
earlier revision's result. The selected integration tests include the existing
four-writer correction acceptance check; this task does not change its
accepted contract. CI runs the full Rust test suite separately.

```sh
python3 -m unittest discover -s tools/sonarqube -p 'test_*.py'
ruff check tools/sonarqube
ruff format --check tools/sonarqube
npm test --prefix tools/governance-validator
npm run validate --prefix tools/governance-validator
```

Tests exercise real Git snapshots and ref updates, private subprocess output,
token isolation, missing/stale evidence, issue multiplicity, and changed-line
coverage. Live analysis is additional integration evidence; unit test success
is never reported as a SonarQube quality-gate pass.
