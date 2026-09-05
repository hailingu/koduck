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

Prerequisites are Python 3.10+, Node 22, Rust 1.95 with `llvm-tools-preview`,
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

- `python3 tools/sonarqube/gate.py pre-commit`: analyze the effective Git index.
  Git's `commit -a` and partial-commit alternate indexes are honored. The normal
  working tree and index are never stashed, reset or staged by the scanner.
- `python3 tools/sonarqube/gate.py pre-push`: consume Git's ref-update lines
  from stdin and check every proposed commit target, including peeled tags.
  Deletions introduce no source and require no analysis. This command never
  performs a push itself.
- `python3 tools/sonarqube/gate.py check --revision HEAD`: check a committed
  revision manually. CI supplies `--base <exact-PR-base-SHA>` as well.

All commits trigger scanning, including documentation-only commits. This avoids
an accidental extension-based bypass when scanner, dependency or build inputs
change. Analysis/verification failure blocks the commit. A completed analysis
with findings may be committed locally for repair; findings, a failed quality
gate or insufficient coverage block **push**, completion and review-ready status.
Do not use `--no-verify` to claim gate success. CI is the merge-time backstop.

## Source identity and increment definition

The scanner uses a private temporary clone with real Git history. For pre-commit
it creates a disposable commit object in that clone containing exactly the
effective index tree. Evidence binds the tree, baseline commit and policy hash;
it never labels the old HEAD as the new source. The index is checked again
before returning. Pre-push compares the proposed commit tree, not the caller's
HEAD. Changing source, baseline or executable policy invalidates evidence.

The baseline is `git merge-base dev <target>` from local history, without an
implicit fetch. CI uses the PR base commit and verifies ancestry. Each analysis
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

## CI and the temporary Docker runner

The `Koduck AI` workflow runs the same gate on a dedicated ephemeral runner with
the label `koduck-sonarqube`. The existing required PostgreSQL check depends on
this gate and fails if it did not succeed, preserving the three existing
required check names. Same-repository PRs only may execute on this local runner;
fork PRs fail the readiness check and require an explicitly trusted workflow.

Run `python3 tools/sonarqube/runner.py` on the machine hosting SonarQube to build
and register a one-job Docker runner using the authenticated `gh` CLI. It uses
an isolated PostgreSQL container, forwards container localhost:9000 to the host
SonarQube, and removes its containers/network afterward. Registration and the
repository readiness variable are part of this explicitly authorized setup.
No host Docker socket or home directory is mounted into the job container.
Run one launcher per queued job. The runner must execute only reviewed/trusted
repository code; it holds a project-scoped analysis token. CI never exposes it
to fork pull requests.

The analysis token reaches the worker over stdin and is stored by the
entrypoint in a mode-0600 file under the runner home; it is never exported
into the job environment, so no workflow step or subprocess inherits it.
`gate.py` loads the file only in the ephemeral CI worker (hooks keep using
the shell export). Residual boundary: the one-job container runs its steps
as a single user, so PR-controlled step code in the same container could
still read the token file while the job runs — the durable mitigation is
that only same-repository, CI-verified code is allowed on this runner, the
token is project-scoped with read-only analysis permissions, and the worker
is destroyed with the job. Per-job token minting requires Sonar
user-administration rights this workflow deliberately does not hold.

Until a runner is started and `KODUCK_SONAR_RUNNER_ENABLED=true`, readiness fails
explicitly. A previously started ephemeral runner does not imply a currently
available runner. This local gate cannot make Git hooks impossible to bypass;
the required CI dependency prevents a skipped local hook from establishing
merge eligibility.

## Verification

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
