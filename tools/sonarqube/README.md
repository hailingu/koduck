# Local SonarQube commit and push gate

The repository owner authorized this workflow directly on 2026-09-05 in task
`01a06ecc-0585-7b63-8311-2022f1a42315`: enable pre-commit analysis without an
ADR/OCR, and permit Git pushes only after incremental SonarQube findings reach
zero. The owner explicitly waived an ADR for implementing this workflow.
Routine installation, disposable test databases and analysis through these
entry points need no ADR, OCR or repeated approval. Other changes retain their
normal governance. This instruction replaces ADR-0015's routing-only activation
and per-operation authorization for this workflow; historical ADR evidence is
not rewritten or presented as approval of this change.

## Installation and use

Run `sh tools/sonarqube/install.sh` in each checkout. It installs pinned c8 and
coverage.py tooling and sets the repository-local `core.hooksPath=.githooks`.
It refuses to overwrite another hook setup. Existing worktrees share Git config;
each worktree must contain this versioned hook directory and the installed tools.

Prerequisites are Python 3.10+, Bash 5+ on `PATH`, Node 22, Rust 1.95 with `llvm-tools-preview`,
`cargo-llvm-cov 0.9.0`, SonarScanner CLI `7.3.0.5189`, and a reachable local
SonarQube supporting Rust, JavaScript and Python (verified on Community Build
`26.8.0.126808`, Rust analyzer `1.8.0.3284`).
On macOS, `brew install bash` supplies Bash 5+ without changing the login shell.
The installer pins `tree-sitter 0.25.2` and `tree-sitter-bash 0.25.1` in the
verification virtual environment; CI installs the same requirements.

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

- `python3 tools/sonarqube/gate.py pre-commit`: analyze the effective Git index.
  Git's `commit -a` and partial-commit alternate indexes are honored. The normal
  working tree and index are never stashed, reset or staged by the scanner.
- `python3 tools/sonarqube/gate.py pre-push`: consume Git's ref-update lines
  from stdin and check every proposed commit target, including peeled tags.
  Deletions introduce no source and require no analysis. This command never
  performs a push itself.
- `python3 tools/sonarqube/gate.py check --revision HEAD`: check a committed
  revision manually. Use `--base <ancestor-SHA>` to select an explicit baseline.

All commits trigger scanning, including documentation-only commits. This avoids
an accidental extension-based bypass when scanner, dependency or build inputs
change. Analysis/verification failure blocks the commit. A completed analysis
with findings may be committed locally for repair; findings, a failed quality
gate or insufficient coverage block **push**, completion and review-ready status.
Do not use `--no-verify` to claim gate success. CI runs the separate checks described below.

## Source identity and increment definition

The scanner uses a private temporary clone with real Git history. For pre-commit
it creates a disposable commit object in that clone containing exactly the
effective index tree. Evidence binds the tree, baseline commit and policy hash;
it never labels the old HEAD as the new source. The index is checked again
before returning. Pre-push compares the proposed commit tree, not the caller's
HEAD. Changing source, baseline or executable policy invalidates evidence.

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
The workflow imports same-snapshot coverage, intersects executable report lines
with `git diff --unified=0 <base> <target>`, and requires at least **80%** coverage
of those changed executable lines. Zero changed executable lines is permitted;
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

`config.json` pins the host, project, tools, source exclusions and time budgets.
Both scans analyze the repository with the same main/test classification.
Rust tests run with cargo-llvm-cov and `--test-threads=1` to bound competition
between database test cases; concurrency inside each test is unchanged.
Node validator tests run with c8, and workflow
tests with coverage.py. Coverage is converted to Sonar generic XML and imported
through `sonar.coverageReportPaths`. Compilation, dependencies, test state and
reports live in disposable checkouts and are removed after evidence is captured.

Shell tests use `shell_coverage.run_shell`: native `sh` for normal regression
runs. After the Python tests exit, a separate scanner-owned fixture driver
executes the versioned Shell entry points with Bash DEBUG probes and writes
the gate's private trace reports. The candidate test process never receives
that report directory, and files it creates are not imported as Shell evidence.
The pinned Bash grammar identifies command locations
in every tracked `.sh` file and `.githooks/` script, including unexecuted files.
Comments, delimiters, and argument/heredoc data are not executable commands.
Fixture copies must match the scanned source bytes before and after execution;
their reports carry source hashes checked again during import. Probes persist
only locations and syntax-token hashes, never command text or arguments. This
also distinguishes multiline substitutions whose commands Bash reports at one
closing line. Ambiguous or unmatched commands remain uncovered. Missing reports,
invalid source syntax, and stale source hashes fail closed. The resulting Shell
LCOV joins the same Git-diff coverage calculation; the 80% threshold is unchanged.

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

## Local scanning and CI

The owner removed the Docker runner build workflow on 2026-09-06. Local
pre-commit and pre-push hooks invoke the installed scanner directly. No custom
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

### Trusted Shell evidence — 2026-09-23

PR 15 review `5286414606` identified that a Python test could write forged
Shell trace JSON into the report directory passed through its environment.
The scanner owns the Shell evidence boundary: candidate tests may exercise the
entry points, but only a separate scanner-controlled fixture run after those
tests exit may contribute Shell hits to the admission report.

| State / precondition | Action / entry point | Observable outcome and invariant | Verification |
| --- | --- | --- | --- |
| Candidate Python test knows the coverage output path | Write a syntactically valid Shell hit for an unexecuted script | The script stays uncovered; candidate-owned files cannot award gate hits | `test_candidate_python_test_cannot_forge_gate_shell_hits` |
| Candidate tests pass and exact versioned Shell entry points are present | Run the scanner-owned hook, entry-point, and installer fixtures after tests | Real execution produces source-bound hits while hook failure, stdin, token selection, and installation outcomes remain correct | `test_trusted_shell_verification_executes_versioned_entrypoints` |
| Scanner runs under system Python without the pinned Bash parser | Start the trusted fixture probes | Use the pinned verification Python for probes; scanner interpreter choice cannot silently remove traces | `test_trusted_shell_verification_executes_versioned_entrypoints` with an unusable scanner interpreter |
| CI installs the parser in the Python user site while fixture `HOME` is isolated | Start a Bash trace probe | Preserve the interpreter's user package base without exposing the real home to fixture scripts | `test_trace_probe_keeps_user_site_with_isolated_fixture_home` |
| Candidate test fails, fixture behavior drifts, or source bytes differ | Attempt Shell report collection | Admission fails; no unchecked or stale report is imported | Python process failure test, fixture checks, `test_changed_source_invalidates_recorded_hits` |

The invariant owner is `python_coverage` for process separation and
`shell_coverage` for source-bound trace generation. The relevant entry points
are the Python test subprocess, trusted fixture driver, and Shell report
collector. This local serial workflow has no concurrent completion or retry
transition; a failed run must be corrected and rerun. The scanned Shell files
are bounded local scripts, so no large-input dimension applies.

Review `5286757657` exposed a remaining trust gap: the candidate Shell process
itself receives the writable trace path and probe helper. A local fixture
invoked the probe for an unexecuted branch and caused `collect()` to mark that
branch covered. The earlier 28/28 result therefore does not prove that
candidate Shell code cannot forge hits. This finding remains open pending a
trusted external evidence boundary or a separately approved gate contract.

Verification passed all 44 Python tests, 184 governance tests, governance
validation, Ruff checks, and the whitespace check. The trusted fixture run
covered all 28 executable lines in the four maintained Shell entry points.
The first sandboxed Python run could not bind its local HTTP fixture; rerunning
with loopback access passed. Commit/push admission and revision-bound review
results are recorded in PR 15.

### Shell coverage remediation — 2026-09-22

The owner requested PR 15 review `5276305522` be fixed and the existing
uncommitted runner cleanup be included, in task
`01a0c834-b8cb-7082-9103-5cd0e65eb3de`. The cleanup completes the recorded
local-only workflow; local subprocess token scrubbing remains active.

| State / precondition | Action / entry point | Observable outcome and invariant | Verification |
| --- | --- | --- | --- |
| A maintained script with two branches | Execute one, then both, through the Shell test runner | Only executed commands receive hits; under-80% coverage rejects and sufficient coverage can pass | `test_tested_shell_only_change_can_pass_and_unexecuted_branch_cannot` |
| Untested executable script or comment-only script | Collect all tracked Shell sources | Untested commands remain uncovered; comments and blank lines contribute no executable lines | `test_unexecuted_and_comment_only_scripts_have_distinct_reports` |
| Source changes after tracing | Import the saved trace | Reject source-hash mismatch; never credit stale source | `test_changed_source_invalidates_recorded_hits` |
| Shell report absent | Compute changed coverage | Reject missing evidence rather than invent zero hits or exempt the file | `test_missing_shell_report_is_not_a_zero_hit_report` |
| Nested hooks, fallback and failure | Execute versioned entry points with fixture subprocesses | Preserve exit status, arguments and stdin; trace only locations, never command text or tokens | Hook process tests and Shell trace regressions |

The shared owners are Shell trace collection, `python_coverage`, and
`changed_coverage`; commit and push consume the same source-bound report.
Tests remain serial. No production concurrency state changes, and no additional
large-data or load-test dimension applies to this bounded parser/tracer change.

Verification on 2026-09-22 passed all 40 Python tests in both native and
instrumented runs, all 184 governance tests, governance validation, Ruff checks,
and the whitespace check. The instrumented hook and installer fixtures covered
all 28 executable lines across the four maintained Shell entry points. Exact
commit/push admission and revision-bound CI/review results are recorded in PR 15.

```sh
export PATH="$PWD/tools/sonarqube/.venv/bin:$PATH"
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
