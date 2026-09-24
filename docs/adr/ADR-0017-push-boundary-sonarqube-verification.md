# ADR-0017: Push-Boundary SonarQube Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-09-24
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-24T03:23:26Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Reason [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Complete, not Blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Complete, not Blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Complete, not Blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Complete, not Blocked
- **Related [Optional]**: `docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md`; `docs/adr/ADR-0016-adr-rejection-reason.md`; `docs/architecture/ADD-0001-ai-service-codex-alignment.md`; owner workflow instruction recorded in `tools/sonarqube/README.md`; current task's request to create the next ADR to reduce testing burden and complexity
- **Architecture Source [Conditionally Required — product demand]**: N/A — owner-requested repository verification governance, not product demand
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

The owner requests an ADR that actually reduces testing burden and complexity,
and selected reducing daily repeated verification as the priority in this task.
ADR-0016 added rejection-reason governance; it did not reduce product tests or
verification executions. The narrowed concurrency design in ADD-0001 constrains
future implementation, but does not change today's verification workflow.

At inspected local `dev` revision
`6bf6a225f2c8641d894d1f0d17919bd46d6b064a`, every Full and Lightweight ADR in
the index has terminal Implementation Status. The blocked OCR does not
participate in ADR serialization. This proposal can therefore be drafted now.

The current local workflow analyzes every commit's effective index and every
push's distinct proposed commit target. Each analysis runs Rust verification
and coverage, then submits a baseline scan and a candidate scan. The database
fixture is entered for each hook invocation. Documentation-only commits take
the same path. A successful pre-commit scan never removes the fresh pre-push
scan, and CI separately runs its required suites.

For N local commits followed by one push of one target, with no extra manual
checks, the current contract therefore schedules N+1 coverage runs and
2(N+1) scanner submissions. These are source-derived operation counts, not
measured elapsed-time or CPU savings. There is no measured wall-time baseline.
The proposal removes the repeated commit-time work and the staging-snapshot
lifecycle that exists solely to support it.

## Scope [Required]

In scope:

- Local SonarQube activation at commit versus push, its installer, command
  dispatch, and removal of exclusively commit-time snapshot/reporting code and
  the evidence reader with no production caller.
- Focused regressions using the existing real-Git and process-boundary fixtures.
- Matching policy and usage updates, including recognition of successful
  pre-push evidence for the same revision's local completion check.
- This record and its index row, delivered through one implementation PR to dev.

Out of scope:

- Product concurrency, persistence, protocol, Rust source or test behavior,
  ADD candidates, and changes to their accepted acceptance checks.
- CI workflow or required-check names, Sonar rules, credentials, profiles,
  coverage thresholds, integration-target selection, and coverage classification.
- Path-based exemptions, cached scan admission, background scans, new queues,
  automatic retries, new dependencies, and replacement test frameworks.
- Trello writes, external publication, release operations, and historical ADR
  rewrites. Approval of this proposal authorizes its implementation scope;
  unrelated external actions keep their existing authorization requirements.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Early analysis feedback versus repeated local verification | Small intermediate commits rerun the entire scan pair and product coverage | Keep focused development tests; move mandatory Sonar analysis to push |
| TN-2 | Faster documentation changes versus classification complexity | A path allowlist or cache introduces new bypass and invalidation cases | Make activation depend only on the Git operation; every pushed target still scans |

### Constraints [Required]

- This is a proposed amendment to the owner's 2026-09-05 commit-scanning
  instruction recorded in the Sonar README and AGENTS.md. It takes effect only
  after this ADR is Accepted and implemented. Routine scan authorization remains.
- Preserve the existing fresh push scan, source identity, zero-incremental-issue
  gate, analysis-bound Quality Gate OK, and 80% changed-product-line coverage.
- Preserve Scope Routing tests, source test-first development, risk coverage,
  review gates, and all three required CI contexts.
- Reuse the existing committed-revision snapshots and test fixtures. Remove
  obsolete commit-only machinery rather than add another verification scheduler.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — no material questions remain in the proposed design. Choosing this
trade-off, including later Sonar feedback, is the decision submitted to @linhai
for approval; drafting it does not imply owner acceptance.

## Decision Drivers [Required]

1. **Immediate reduction**: Fewer expensive executions on the current workflow,
   with deterministic operation-count acceptance criteria.
2. **Less maintained state**: Eliminate the index-snapshot lifecycle and its
   exclusive tests; keep one committed-revision verification path.
3. **Preserved admission assurance**: Each actual push remains freshly verified
   regardless of file types or previously successful analysis.

## Options Considered [Required]

### Option: Keep mandatory scanning at commit and push

Pros:

- Sonar feedback arrives before a local commit is created.

Cons:

- Retains all repeated execution and staged-index lifecycle code; provides no
  reduction for the owner's current request.

### Option: Add documentation exemptions or reuse cached scans

Pros:

- Could reduce both commit and push work for eligible revisions.

Cons:

- Requires classification or freshness rules for source, analyzer, policy,
  baseline, and external state; adds implementation and regression cases.

### Option: Require Sonar at push and retain an explicit manual check

Pros:

- Removes all automatic commit-time coverage/scans without path classification.
- Removes an entire staging-snapshot lifecycle while keeping fresh push evidence.

Cons:

- A local commit may contain issues discovered only at push or manual check.
- Every push, including a documentation-only push, retains the full scan cost.

## Decision [Required]

**Selected option**: Require Sonar at push and retain an explicit manual check.

**Rationale**: Changing one activation boundary reduces repeated work for source
and documentation alike. The existing committed-revision path already owns
the necessary admission invariants. Removing the other path reduces the code
and test scenarios that must be maintained.

- **PV-1 — Commit behavior**: The installed repository workflow MUST NOT invoke
  Sonar, coverage, Cargo verification, the database fixture, token loading, or
  the Sonar lock merely because Git creates a local commit. Remove the versioned
  pre-commit hook; do not replace it with a no-op hook or success-evidence stub.
  The Python CLI MUST accept only `pre-push` and `check`;
  `python3 tools/sonarqube/gate.py pre-commit` MUST exit 2 through argument
  rejection before credentials, database creation, lock acquisition, or scanning.
  This pre-credential guarantee applies only to the direct Python CLI. Calling
  the existing shell wrapper with the unsupported `pre-commit` argument may
  load credentials before Python rejects it; changing that wrapper is outside
  scope. Neither invocation is successful verification.
- **PV-2 — Fresh push admission**: The existing pre-push hook and default/manual
  shell entry points MUST continue to invoke the canonical gate. For every
  distinct non-deletion proposed commit target, including a peeled tag, pre-push
  MUST run fresh committed-snapshot coverage and a baseline/candidate scan pair.
  Verify the proposed object, not the checkout HEAD. File type, a previous scan,
  and saved evidence MUST NOT skip this work. Ref deletions require no analysis;
  malformed ref input is rejected. The local-dev merge-base and explicit manual
  ancestor-base rules remain unchanged.
- **PV-3 — Existing gate and failure ownership**: Push and manual checks MUST
  preserve exact tree/base/policy binding, zero incremental unresolved findings,
  Quality Gate OK, at least 80% changed executable product-line coverage, and
  the existing proven-zero-line case. Missing reports or mismatched evidence
  MUST fail. Existing token containment, same-snapshot coverage, timeout,
  cancellation, cleanup, and host-local scan locking remain unchanged. A target
  whose verification fails MUST NOT emit its admission line. A later target's
  failure or remote Git rejection does not invalidate an earlier target's
  successful verification under PV-5. Neither establishes overall push success;
  local commits remain available for correction and every later push scans fresh.
- **PV-4 — Installation**: The installer MUST enable the executable pre-push
  hook through the existing `core.hooksPath=.githooks`, without referencing a
  removed pre-commit file. Reinstallation MUST succeed idempotently. Its current
  refusal to overwrite conflicting hook configuration or unmanaged hook files
  MUST remain. Existing managed checkouts adopt the changed versioned hooks
  when they check out this implementation; no user hook is deleted.
- **PV-5 — Evidence and development checks**: Local commits MUST NOT be claimed
  as Sonar-verified. Focused tests during development and all applicable routed
  tests, lint, governance, acceptance, review, and CI gates remain required.
  Each Scope Routing cell that currently requires
  `python3 tools/sonarqube/gate.py check --revision HEAD` MUST instead state:
  "`python3 tools/sonarqube/gate.py check --revision HEAD`, or matching successful
  pre-push evidence as defined in Local SonarQube Feature Completion Gate."
  This alternative MUST also be stated in Verification And Completion Evidence;
  all other routed commands remain exact and mandatory.

  After `require_pass` succeeds, `gate.check_revision` MUST emit and flush one
  complete stdout line in this exact field order, using the validated identity
  and metrics from the same record that passed `require_pass`:

  `Sonar push admitted: <revision> analysis=<analysis-id> tree=<tree> base=<base> policy=<policy> new_issues=<n> quality_gate=<status> covered=<c> coverable=<d>`

  `<revision>` is the resolved commit and `<analysis-id>` is its nonempty
  analysis ID. The tree/base/policy fields are the identity passed to
  `require_pass`; new_issues, quality_gate, covered and coverable are the
  identically named record values it accepted. These supply the issue count,
  Quality Gate and coverage numerator/denominator required by AC-7 and AGENTS.md,
  including the proven-zero-line case without dividing by zero.
  Capture the complete line from the actual canonical pre-push invocation using
  the combined stdout and stderr of `git push`: the gate itself writes stdout,
  but Git routes hook stdout to its stderr. The README MUST state this capture
  rule. Its revision/tree/base/policy MUST equal the current values
  recomputed by the read-only procedure below. A missing field, the old shorter
  line, absent admission output, or any mismatch does not qualify. Do not read
  persisted evidence or compute an evidence filename to qualify this line;
  failed results may still be stored before `require_pass`.

  Admission is per target. An earlier target's line remains valid when a later
  target fails, or when the remote rejects the Git push, provided that target's
  identity still matches. Other targets without their own qualifying lines
  remain unverified. Overall hook or Git exit status is not the criterion for
  this target-level evidence and MUST NOT be inferred from the line. A
  deletion-only invocation cannot supply an admission line.

  Matching successful pre-push evidence MAY satisfy the Sonar portion of local
  completion/review-ready verification; a second manual `check` solely to
  duplicate that result is not required. Record the actual invocation,
  admission line and identity-comparison result, never a manual command that
  was not run. Report overall push success or failure separately when known.
  Otherwise run the existing manual check before claiming Sonar completion.
  This permission reuses evidence only for reporting; every later push still
  scans afresh, and the admission message is not proof of remote Git publication.
- **PV-6 — Maintained scope**: Remove commit-only dispatch, index snapshot and
  index-recheck helpers, commit-only success-with-findings reporting, and
  `gate.load_evidence`, which has no production caller and is not needed for
  PV-5's self-contained admission-line rule. Migrate shared analysis fixtures to the
  existing committed-revision snapshot helper. Tests of evidence persistence
  MUST inspect the JSON written by `store_evidence` at `evidence_path`; retain
  identity-key separation assertions and remove only loader-specific cases.
  Move the caller-worktree/index preservation assertions from
  `test_gate.SnapshotTests.test_snapshot_contains_index_only_and_preserves_worktree`
  into `test_gate.SnapshotTests.test_revision_snapshot_preserves_caller_state`,
  using `revision_snapshot` with staged, unstaged and untracked caller changes.
  Assert the requested committed snapshot's contents and unchanged caller HEAD,
  index tree and working-tree contents/status after cleanup. Remove the obsolete
  index-only content assertion and the entire
  `test_gate.SnapshotTests.test_index_change_invalidates_completed_scan` case.
  Retain assertions for shared analysis, snapshot integrity, push admission and
  manual checks. Do not
  add a cache, path classifier, background worker, retry loop, new dependency,
  or product/CI change. Test the activation boundary with real Git and existing
  doubles at expensive I/O; routing regressions require no live Sonar or database.

  Retained implementation and policy files changed by this ADR, including the
  modified tests and the pre-push hook, MUST cite
  `ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md` at their first
  legal comment position, preserving required headers and existing valid ADR
  citations. Use shell/Python comments or Markdown HTML comments as applicable.
  The removed pre-commit hook needs no new marker. Python/shell markers under
  `tools/sonarqube` may change the policy hash; that is expected and requires
  fresh evidence under the resulting policy, not preservation of the old hash.
- **PV-7 — Observable reduction**: For 1 and 3 local commits followed by one
  push of one target, with no separate manual check, the revised workflow MUST
  invoke coverage once, submit exactly two scans, and enter the database-fixture
  context once per workload. Commit operations contribute zero to all three.
  Compared with the inspected baseline,
  coverage runs fall from 2 to 1 and 4 to 1 respectively; scan submissions fall
  from 4 to 2 and 8 to 2; fixture-context entries fall from 2 to 1 and 4 to 1.
  A context entry is not necessarily container creation when an isolated
  database URL is supplied. Derive the N=1 and N=3 totals from AC-1's zero-cost
  commit result and AC-2's measured single-target push counts; do not add
  separate repeated-commit workload tests or use elapsed-time thresholds.

### Consequences [Required]

Positive:

- The defined workloads remove 50% and 75% of local coverage executions and
  scan submissions respectively, without reducing the checks on a pushed target.
- Local commits no longer need a running Sonar server or PostgreSQL fixture.
- Verification no longer maintains staged-index snapshot/recheck semantics.

Negative:

- Push may reveal problems accumulated across several local commits.
- Source tests and CI duration do not automatically decrease; product concurrency
  simplification remains separate work. No elapsed-time reduction is promised.
- A deletion-only push still requires the token and enters the existing Sonar
  lock and database fixture before dispatch, although it submits no scan.
  Removing that cost is outside this ADR's scope.

Mitigations:

- Continue focused tests while editing and allow manual Sonar checks when early
  analysis feedback is useful. Fix locally and retry the push with fresh evidence.
- Preserve all existing required CI contexts and admission thresholds; disclose
  the existing limitation that CI cannot prove local hooks were not bypassed.

## Implementation Plan [Required]

**Complete task outcome**: One implementation PR makes local commits free of
automatic Sonar work, retains fresh push/manual admission, removes exclusively
staged-index machinery, and proves the PV-7 reduction with updated policy.

**Primary implementation boundary**: Local SonarQube verification orchestration
in `tools/sonarqube`; hook/installer and policy changes support that boundary.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Move mandatory analysis to push, remove commit-only machinery, and prove preserved admission plus reduced execution | PV-1 through PV-7 at the affected paths below | Complete | Delivered through PR #19 (https://github.com/hailingu/koduck/pull/19), merged into `dev` as `9ba3039c5f1213ac70cf8e70505480420d472f1f` on 2026-09-24T06:53:44Z. Implementation commit `bcaeb412850e4ee3ca872691ede46bc358662e09`: `.githooks/pre-commit` removed; CLI narrowed to `pre-push`/`check`; post-`require_pass` admission line extended and flushed; installer chmods only `pre-push`; index snapshot/recheck helpers, commit-only reporting and `gate.load_evidence` removed; shared fixtures migrated to `revision_snapshot`; README/AGENTS policy synchronized with markers. AC-1 through AC-7 all Pass; the exact merged head `74a948d7c732aac2c1b8ccdbb6ab12d8813884e1` carries green required CI and Codex review coverage. Red phase on the pre-implementation tree: 8 failing items (`test_retired_pre_commit_mode_exits_2_before_gate_resources` exit 1≠2, old installer chmod failure in `test_local_commits_activate_no_gate_resources` and `test_installation_activates_push_hook_and_preserves_conflicting_hooks`, short admission line and failed-check cases); green phase 43/43 OK. The local commit itself ran no Sonar work, demonstrating PV-1 in this repository; the post-merge deletion-only branch push entered the fixture but submitted no scan, matching PV-2 |

**Affected paths**: `.githooks/pre-commit` (remove);
`.githooks/pre-push` (governed-file marker only);
`tools/sonarqube/install.sh`; `tools/sonarqube/gate.py`;
`tools/sonarqube/git_snapshot.py`; `tools/sonarqube/test_hooks.py`;
`tools/sonarqube/test_entrypoints.py`; `tools/sonarqube/test_gate.py`;
`tools/sonarqube/test_flow.py`; `tools/sonarqube/test_reports.py`;
`tools/sonarqube/test_runtime.py` (evidence-persistence assertions);
`tools/sonarqube/README.md`; `AGENTS.md`; this ADR; `docs/adr/INDEX.md`.
The retained implementation/policy files above receive the PV-6 marker; the ADR
and index use their normal record/reference format. The pre-push hook receives
no executable change. The shared shell entry, scan runtime, API, coverage and
database modules are inspected/tested dependencies with no authorized edits.
The generic AGENTS.template.md contains no concrete Sonar activation policy and
needs no change.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

All inspected source below is represented by
`6bf6a225f2c8641d894d1f0d17919bd46d6b064a`.

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| .githooks/pre-commit | Hook entry point | `exec sh "$(git rev-parse --show-toplevel)/scripts/sonar-quality-gate.sh" pre-commit` | Remove the automatic commit activation | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| .githooks/pre-push | Hook entry point | `exec sh "$(git rev-parse --show-toplevel)/scripts/sonar-quality-gate.sh" pre-push` | Add the PV-6 marker after the shebang; preserve executable behavior | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/install.sh | core.hooksPath; managed-hook executable setup | `chmod +x "$root/.githooks/pre-commit" "$root/.githooks/pre-push"` | Install only the push hook; preserve conflict checks; add PV-6 marker | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/gate.py | gate.main; gate.report_result; gate.load_evidence; gate.store_evidence; gate.evidence_path; gate.check_revision; gate.analyze; gate.policy_id | N/A — stable symbols suffice | Remove commit dispatch/reporting and unused loader; extend and flush the post-require_pass admission line with tree/base/policy and the accepted new_issues/quality_gate/covered/coverable metrics; retain persistence and admission predicates; add marker with expected policy-hash change | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/git_snapshot.py | git_snapshot.index_snapshot; git_snapshot.require_index; git_snapshot.git; git_snapshot.revision_snapshot; git_snapshot.push_revisions | N/A — stable symbols suffice | Remove index-only helpers/argument; reuse committed snapshots and ref parsing; add PV-6 marker | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/test_hooks.py; tools/sonarqube/test_entrypoints.py | test_hooks.HookTests; test_entrypoints.EntrypointTests | N/A — stable types suffice | Exercise real installation/commit hooks and entrypoint dispatch with controlled expensive boundaries | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/test_gate.py; tools/sonarqube/test_flow.py; tools/sonarqube/test_reports.py | test_gate.SnapshotTests.test_snapshot_contains_index_only_and_preserves_worktree; test_gate.SnapshotTests.test_index_change_invalidates_completed_scan; test_flow.FlowTests; test_reports.ScanTests | N/A — stable symbols suffice | Migrate caller-state preservation to planned test_revision_snapshot_preserves_caller_state; remove index-only invalidation case; retain shared committed-snapshot and analysis assertions | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/test_runtime.py | test_runtime.EvidenceTests.test_evidence_lookup_is_bound_to_tree_base_and_policy | N/A — stable symbol suffices | Assert persisted JSON and identity-key separation without the retired loader; add PV-6 markers to all modified tests | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| AGENTS.md | Work Coordination, Review, And Delivery Sources Of Truth — CI correspondence exception; Execution Workflow step 10; Scope Routing; Verification And Completion Evidence; Local SonarQube Feature Completion Gate | N/A — contract anchors suffice | Replace commit-and-push enforcement with push enforcement; put PV-5 alternative in every affected routing cell and evidence rule; add marker | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |
| tools/sonarqube/README.md | Document title; opening owner-authorization paragraph; Installation and use; Source identity and increment definition; Execution and failure contract; Local scanning and CI | N/A — contract anchors suffice | Rename to push gate; preserve original authorization as history with explicit ADR-0017 amendment; synchronize active guidance and add marker | 6bf6a225f2c8641d894d1f0d17919bd46d6b064a |

Start with the smallest real-Git regression showing a local commit invokes
the old expensive path; record its expected failing command/result in T-1.
Then remove activation, update the installer/CLI and migrate shared fixtures.
Use the existing Python unittest suite; add only the cases selected below,
reusing current passing assertions where they cover the same invariant.
Document review is semantic; do not test ordinary policy prose as opaque text.

### Scoped Verification Procedure [Required]

AC-6 uses `git diff --name-status 6bf6a225f2c8641d894d1f0d17919bd46d6b064a HEAD`
to check that the committed implementation touches only Affected paths, then
inspects the named touchpoints for PV-1 through PV-6. Inspect `gate.main`, its
module signal handler and `gate.project_lock` for unchanged push/manual context
lifetimes, locking and cleanup. The unchanged `scan_runtime.run`, `Sonar.wait`,
`database_fixture`, coverage modules and config establish unchanged timeouts,
cancellation, resource limits and thresholds. The existing database-cleanup
regression must pass. Check the retained-test map, marker placement, and policy
anchors, including PV-5 cases with missing/short admission output, identity
mismatch, a matching line, and an earlier admitted target followed by another
target's failure or a remote rejection. No fresh review pass is
implied by this future implementation check in the current drafting task.

For PV-5 identity comparison, run this read-only command from the repository
root with the workflow's required Python 3.10+ (the `python3` on PATH),
replacing the `HEAD` argument with the admitted target's exact commit.
It uses existing helpers, reads no credentials or stored evidence, and starts
no scanner or database. Compare its four output fields with the admission line;
the analysis ID is retained from the captured post-`require_pass` output.
Include this procedure in the Sonar README during implementation.

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

For AC-2's in-process single-target count case, keep `gate.main`,
`check_revision`, `analyze`, `require_pass`, Git operations and
`revision_snapshot` real. Replace `gate.preflight` with a no-op,
`gate.coverage` with a fixture `CoverageResult`, `gate.scan` with counted
baseline/candidate results, and the `gate.Sonar` factory with the existing
`test_flow.SonarFixture`. Replace `gate.database_fixture` with a counted null
context and `gate.project_lock` with a null context. Supply argv/stdin and
capture stdout in-process; no installed scanner, live server or database is
needed. Shell-hook subprocess tests remain separate from these Python doubles.

Run the routed commands once for the implementation revision:

```sh
python3 -m unittest discover -s tools/sonarqube -p 'test_*.py'
ruff check tools/sonarqube
ruff format --check tools/sonarqube
npm test --prefix tools/governance-validator
npm run validate --prefix tools/governance-validator
git diff --check
```

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: Apply the hook removal, installer, CLI, tests and
policy in one change after approval. Re-run the existing installer in a disposable
fixture to verify both first install and already-managed upgrade. Existing
managed worktrees follow their own checked-out hook version. Stop if push
admission can bypass fresh analysis or a user hook would be overwritten.
Rollback restores the coupled pre-commit hook, CLI/index helpers, installer and
policy from the parent revision through the applicable authorized change; do not
restore only the hook against a CLI that rejects it. No data migration applies.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no exception is requested. The design removes existing code and reuses
existing boundaries. Reassess changed units against the common engineering
standard before completion; a newly required exception is approval-sensitive.

### Key State And Invariant Matrix [Required]

The owner of every row is the local verification orchestrator. Entry points are
installed Git hooks, the installer, and the gate CLI. Real Git fixtures own ref
and snapshot semantics; doubles replace only external scanning/coverage/database
work in the focused routing tests.

| ID | Precondition or state | Action or transition | Expected observable outcome | Invariant | Test or verification gap |
| --- | --- | --- | --- | --- | --- |
| S-1 | Managed hooks; source or documentation edit; scanner/database/token unavailable | Create a local Git commit | Commit succeeds; zero gate/coverage/scanner/database activation | Commit is a local checkpoint, not verification evidence | AC-1 |
| S-2 | Direct Python CLI invoked with retired pre-commit mode | Parse arguments without the shell wrapper | Exit 2 before external I/O | Unsupported mode never grants verification; shell token-loading order is outside this guarantee | AC-1 |
| S-3 | Proposed branch or tag differs from checkout HEAD; prior successful evidence exists | Invoke pre-push with real Git ref-update input | Each distinct target scans afresh and binds its own tree/base/policy | Only actual pushed objects can be admitted | AC-2 |
| S-4 | New findings, non-OK gate, low coverage, missing report or evidence mismatch | Attempt admission, correct the fixture, and retry | First check fails; corrected retry performs fresh analysis and passes | Failure and stale results never authorize a push | AC-3 |
| S-5 | New checkout, already-managed hooks, or conflicting user hook setup | Run installer twice or attempt conflicting installation | Idempotent managed setup; conflict rejected without modification | Installation never overwrites user hook ownership | AC-4 |
| S-6 | AC-1 establishes zero commit contribution; AC-2 measures one-target push | Derive totals for N=1 and N=3 without new workload executions | Coverage=1, scans=2, database-fixture entries=1 for either N | Work scales with distinct pushed targets, not intermediate commits | AC-5 |
| S-7 | Missing/short admission line or changed target/tree/base/policy; separately a matching line followed by another target failing or remote rejection | Recompute current identities with the documented read-only command; evaluate per-target reporting eligibility | Only a complete matching line qualifies for its target; later unrelated failure does not invalidate it; no overall push success is inferred | Stored files and aggregate exit codes do not prove target admission; reporting never skips a later scan | AC-2, AC-3, AC-6 |

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| PV-1 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Commit has no automatic heavy work; direct Python retired mode exits 2 before I/O | AC-1 | Real commits with PATH sentinels; direct Python CLI rejection tested separately from shell wrapper |
| PV-2 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Every proposed distinct non-deletion commit scans fresh; preserve refs/bases/manual path | AC-2 | Real Git branch/tag/multi-ref/deletion fixtures, existing fresh-scan regression, explicit manual base cases |
| PV-3 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Preserve gate predicates and process protections; emit no admission for a failed target | AC-2, AC-3, AC-6, AC-7 | Negative admission and recovery cases, partial multi-target outcome, named-symbol scope inspection and live implementation-revision gate |
| PV-4 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Installer enables retained hook idempotently and refuses conflicts | AC-4 | Real Git configuration, executable hook invocation, first install/reinstall/conflict fixtures |
| PV-5 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Routing permits a full post-require_pass target admission line with recomputed matching identities, independently of aggregate push exit | AC-2, AC-3, AC-6 | Assert expanded line fields and failure emission behavior; execute read-only identity comparison and inspect all reporting eligibility cases |
| PV-6 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | Remove commit-only lifecycle and unused loader; migrate caller-state preservation, retain shared/persistence assertions and add markers | AC-2, AC-6 | Real revision_snapshot caller-state case plus Scoped Verification Procedure, retained-test map and routed suite |
| PV-7 | docs/adr/ADR-0017-push-boundary-sonarqube-verification.md — Decision | For either N, totals are coverage=1, scans=2, database-fixture entries=1 | AC-1, AC-2, AC-5 | Derive totals from zero commit contribution and in-process single-target push counters; no duplicate workload tests |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | Applicable — more than one ref, stale saved result, or checkout HEAD different from a pushed target | Git ref dispatch and committed-snapshot analysis | Real Git multi-ref/peeled-tag fixtures and fresh-scan regression | Every distinct non-deletion target checked once per push; unchanged serial lock surrounds scans; saved evidence skips none | AC-2, AC-6 | Pass | `test_failed_later_target_never_retracts_the_earlier_admission_line` (two-target push, second failing, exactly one per-target line, fresh scans per target), retained `test_push_checks_proposed_object_not_current_head`, `test_cached_result_never_skips_fresh_push_scan`; `gate.main` still wraps dispatch in `project_lock()`; suite 43/43 OK |
| timeout and deadline | N/A — removing commit activation introduces no timer or wait; push/manual time budgets are unchanged | Existing scan_runtime.run and Sonar.wait | Structured implementation diff review explicitly checks timers, subprocess limits, compute settlement, and unchanged config | No timeout, deadline, wait, or retry policy change | AC-6 | N/A — no timing-policy change | AC-6 structured review of the implementation diff: `scan_runtime.run`, `Sonar.wait`, `rust-coverage.sh` and `config.json` are untouched; the only `gate.py` changes remove the pre-commit branch and extend post-admission output, adding no timer or wait |
| cancellation and interruption | N/A — retired commit path owns no new operation; retained push/manual cleanup and signal handling are unchanged | Existing gate signal handler, scan_runtime.run, and database_fixture | Structured diff review explicitly checks signal handling, context-manager nesting and process/container cleanup; existing cleanup regression | No new cancellation path or weakened cleanup; existing cleanup regression passes | AC-6 | N/A — no cancellation-policy change | AC-6 structured review: the `__main__` signal handler, `project_lock` context and `database_fixture` nesting are unchanged; only the pre-commit dispatch branch was removed from `gate.main`; existing cancellation/cleanup regressions pass in the 43-test suite |
| resource bounds and backpressure | Applicable — intermediate commits unnecessarily create verification processes and fixture lifetimes | Hook activation and gate orchestration | AC-1 subprocess sentinels plus AC-2 in-process push counters; derive workload totals in AC-5 and inspect retained serialization | Commits activate zero heavy work; one-target push activates one coverage run, two scans and one database-fixture context; no background work or queue | AC-1, AC-2, AC-5, AC-6 | Pass | AC-1 sentinel commits activate zero gate resources; AC-2 counted push workload is exactly {coverage 1, scans 2, fixture entries 1}; no cache, classifier, queue, worker or retry added (diff adds none) |
| framework or trust-boundary rejection | Applicable — retired CLI mode, malformed ref input, unsupported baseline, failing analysis or wrong evidence | CLI, Git hook input, and require_pass | CLI/Git fixtures plus existing negative gate cases and corrected retry | Unsupported mode exits 2; invalid refs/bases and failing/mismatched evidence reject; valid corrected target passes fresh analysis | AC-1, AC-2, AC-3 | Pass | Direct CLI `pre-commit` subprocess exits 2 with no evidence; retained `SONAR_PUSH_INPUT`/deletion cases and manual-base ancestry check green; AC-3 negative matrix rejects with no admission line and corrected retry analyzes afresh |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Local commit no longer activates Sonar work | Real Git fixture with installed hooks; source and Markdown commits; unset token and PATH sentinels for Python gate startup, zsh, Cargo, Docker and scanner; separately invoke gate.py pre-commit using the actual Python executable | HookTests run commits as subprocesses and inspect PATH sentinel logs; EntrypointTests exercise the direct Python CLI separately, without the shell wrapper | Commits exit 0, contain intended contents and activate none of the sentinels or gate resources; direct Python retired mode exits 2 before credentials/lock/database/scan and creates no analysis evidence; no pre-credential assertion applies to the shell wrapper | Commit/CLI exit codes and sentinel records; no in-process patch is assumed to affect a child process | Pass | Red on the pre-implementation tree: direct `gate.py pre-commit` subprocess exited 1 (old CLI accepted the mode and entered the credential path); installer chmod of the removed hook failed both hook fixtures. Green after implementation: 43/43 OK. Fixture commits with PATH sentinels for python3, zsh, cargo, docker and sonar-scanner and no token exit 0, contain the intended source and Markdown contents, and write neither `args` nor `sentinels`; the stripped-token direct `gate.py pre-commit` subprocess exits 2 and leaves no `.git/sonarqube` evidence (`test_local_commits_activate_no_gate_resources`, `test_retired_pre_commit_mode_exits_2_before_gate_resources`) |
| AC-2 | T-1 | Push/manual dispatch preserve identity, fresh analysis and caller state | Real Git ref/base fixtures and saved evidence; staged/unstaged/untracked caller changes; multi-target sequence with first target passing and second failing | Replace old pre-commit entrypoint case with gate.main() pre-push argv and real stdin; retain manual check; execute the real-analyze single-target case with the complete preflight/coverage/scan/SonarFixture/database/lock double set listed in Scoped Verification Procedure; separately run shell propagation tests and test_revision_snapshot_preserves_caller_state | Each distinct non-deletion target checked once with matching revision/tree/base/policy; passing target emits and flushes the full PV-5 line with identical validated-record metrics only after require_pass; single-target counts 1/2/1; later failing target emits no line and cannot retract the first; invalid refs/bases reject, deletion scans zero, valid manual check passes; committed snapshot correct and caller HEAD/index/worktree unchanged; shell stdin/failure propagate | Scenario-to-test map, complete admission lines, in-process counters, caller-state comparisons and separate shell-hook result | Pass | Doubles exactly per Scoped Verification Procedure (real `gate.main`/`check_revision`/`analyze`/`require_pass`/Git/`revision_snapshot`; no-op preflight, fixture `CoverageResult`, counted baseline/candidate scans, `SonarFixture`, counted null database context, null lock): single-target counters exactly {coverage 1, scans 2, fixture 1}; admission line matches the exact nine-field order with the accepted record's metrics (`test_single_target_push_runs_one_coverage_two_scans_one_fixture`, `test_pre_push_emits_one_complete_admission_line_for_admitted_target`); two-target push with a failing second target emits exactly one unretracted line (`test_failed_later_target_never_retracts_the_earlier_admission_line`); `EntrypointTests.test_push_and_check_record_the_requested_tree` drives `gate.main()` pre-push argv with real stdin; `test_revision_snapshot_preserves_caller_state` proves unchanged caller HEAD/index/worktree; shell hook propagates exit 23 and stdin (`test_pre_push_propagates_gate_failure_and_push_stdin`); invalid-ref, deletion and peeled-tag cases retained green; suite 43/43 OK |
| AC-3 | T-1 | Failed target admission supplies no successful target evidence and recovery scans afresh | Independently use one new issue, non-OK gate, 79/100 coverage, absent report or wrong tree/base/policy; then zero issues, OK, 80/100 and matching identity; include proven zero-line case | Existing negative gate/report cases plus check_revision recovery case; capture stdout and inspect persistence in the retained persistence tests only | Invalid target rejects with existing diagnostic and no admission line regardless of a stored result; valid target emits the complete matching PV-5 line; corrected retry invokes analyze afresh | Gate/report/flow results and corrected-retry output; stored records are not used for PV-5 eligibility | Pass | `test_failed_check_emits_no_line_and_corrected_retry_analyzes_fresh` over {new_issues=1, quality_gate=ERROR, covered=7/coverable=10, tree mismatch}: each variant raises the owned diagnostic with zero admission lines while `store_evidence` still persists the failing record; the corrected retry invokes `analyze` afresh (two calls) and emits exactly one line; retained `AdmissionTests` field matrix, zero-coverable and missing-report cases green |
| AC-4 | T-1 | Installation preserves hook ownership and activates push-only workflow | Fresh fixture checkout; same checkout already configured to .githooks; different hooksPath; unmanaged pre-commit or pre-push hook under default hooks | Existing installer shell fixture executed with real Git config, then invoke retained executable hook | First install and reinstall exit 0, core.hooksPath equals .githooks and pre-push runs; no removed-file chmod failure; each conflicting setup exits 1 with SONAR_EXISTING_HOOKS and preserves original config/files | Installer scenario report and before/after ownership evidence | Pass | `test_installation_activates_push_hook_and_preserves_conflicting_hooks`: first install and reinstall exit 0 with `core.hooksPath=.githooks`; installed `pre-push` is executable and returns sentinel 23 with `pre-push` argv and stdin; a different hooksPath, and unmanaged `.git/hooks/pre-commit` and `.git/hooks/pre-push` each exit 1 with `SONAR_EXISTING_HOOKS`, preserving the original files; no chmod references the removed pre-commit file |
| AC-5 | T-1 | Derived workload totals match PV-7 without extra regression executions | Passing AC-1 zero-contribution result and AC-2 single-target push counters; no separate manual check | Reuse those results and compute N × commit contribution + one push for N=1 and N=3; compare with source-derived baseline N+1 coverage/fixture entries and 2(N+1) scans | For each N: coverage=1, scans=2, fixture-context entries=1; reductions 50% at N=1 and 75% at N=3; no additional repeated-commit workload test | Count table citing the exact AC-1 and AC-2 results, not a new test run or timing claim | Pass | N=1: 1×(0,0,0) commit contribution (AC-1 sentinels) + (1,2,1) push (AC-2 counters) = coverage 1, scans 2, fixture entries 1, versus baseline (2,4,2) = 50% reduction each. N=3: 3×(0,0,0) + (1,2,1) = (1,2,1), versus baseline (4,8,4) = 75% reduction. No separate repeated-commit workload test was added and no elapsed-time claim is made |
| AC-6 | T-1 | Implementation and policy match the scoped decision | Committed implementation diff against 6bf6a225f2c8641d894d1f0d17919bd46d6b064a and retained-test map | Execute Scoped Verification Procedure and its listed routed commands; inspect the named symbols and policy anchors | Only Affected paths changed; PV-1 through PV-6 implemented, including markers and reporting eligibility; named retained protections unchanged; all listed commands exit 0 | Path diff, symbol/policy dispositions, test map and command results | Pass | On committed revision `bcaeb412850e4ee3ca872691ede46bc358662e09`: `git diff --name-status 6bf6a22 HEAD` lists exactly the Affected paths (`.githooks/pre-commit` deleted; `pre-push`, installer, `gate.py`, `git_snapshot.py`, six modified tests, Sonar README, `AGENTS.md`, this ADR and the index changed); markers verified at each first legal comment position (`.githooks/pre-push:2`, `install.sh:2`, `gate.py:4` after the module docstring, `git_snapshot.py:3`, all six modified tests after their docstrings, `README.md:1`, `AGENTS.md` marker list); `gate.main` retains the `project_lock()`/`database_fixture()` lifetimes and the SIGTERM handler; `scan_runtime`, `sonar_api`, `coverage_report`, `postgres_fixture`, `config.json` and `scripts/sonar-quality-gate.sh` are untouched; routed commands on the committed revision: 43/43 unittest OK, `ruff check tools/sonarqube` clean, `ruff format --check` reports all 15 files formatted, governance 208/208 tests and validation passed, `git diff --check` clean |
| AC-7 | T-1 | Real implementation revision satisfies the retained admission gate | Committed implementation revision, isolated fixture database and owner-authorized scanner environment | Pre-push evidence qualifying per target under PV-5 plus its read-only identity comparison, or python3 tools/sonarqube/gate.py check --revision HEAD | Zero incremental issues, Quality Gate OK, and at least 80% changed product-line coverage or proven-zero-line case; pre-push alternative has the complete admission line and matching recomputed revision/tree/base/policy; overall Git push result reported separately | Actual invocation and combined-output admission line with identities, analysis ID, new_issues, quality_gate, covered and coverable; required CI green and exact-revision review before review-ready | Pass | Local admission gate satisfied on `bcaeb412850e4ee3ca872691ede46bc358662e09` by the canonical manual check `sh scripts/sonar-quality-gate.sh check --revision HEAD` (exit 0), which printed the complete line `Sonar push admitted: bcaeb412850e4ee3ca872691ede46bc358662e09 analysis=450ed1f9-cc2f-4f5c-b4c6-5c339303a7b3 tree=6fcab9909b759247f41002dafb32216ced634a10 base=988a920906c07d5d923814f694b931021cd1bc51 policy=27cba4047ee56bbaa921a14e8c9f738c9872d59eb64e4bca77f5eb6d6fff418c new_issues=0 quality_gate=OK covered=0 coverable=0`; the read-only identity procedure returned the identical revision/tree/base/policy; covered=0/coverable=0 is the proven-zero-line case because the diff touches only `tools/`, `.githooks/`, `AGENTS.md` and `docs/` with Rust sources unchanged. A first local attempt on superseded unpushed revision `55628b0` failed `require_pass` with `SONAR_QUALITY_GATE_FAILED` and one incremental issue `python:S5778` at `tools/sonarqube/test_flow.py` (two invocations inside one `assertRaises` block); the direct Sonar remediation hoisted the `SonarFixture()` construction out of the guarded block, the commit was amended before any push, and the failed candidate analysis remains visible on the dashboard. Both PR #19 pushes also carried matching PV-5 pre-push admission lines (`778df7484c8a0f282ba2277d317cd7eb73fbf5ac` analysis `dd0525af-bafe-4f64-886b-6fc19741e8af`, `74a948d7c732aac2c1b8ccdbb6ab12d8813884e1` analysis `24931ab9-3e20-4db9-ad85-88ca5ccb47f1`; both `new_issues=0 quality_gate=OK covered=0 coverable=0`) with read-only identity recomputations matching all four fields. Review-ready portion satisfied on the exact merged head `74a948d`: all three required CI checks green (after one rerun of a timing-flaked product concurrency test on a Rust-identical diff, recorded in PR #19) and Codex automatic review round 2 on that commit reported no findings (round 1's single P2 on `778df74` was addressed in `74a948d` with an in-thread reply and resolution); PR #19 merged into `dev` as `9ba3039c5f1213ac70cf8e70505480420d472f1f` |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply. AC-7 cannot be satisfied by unit doubles.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Eligible non-author @linhai, timestamp, and exact Approval Evidence: Approve recorded | Metadata and user approval | Complete | @linhai explicitly approved ADR-0017 in this task; recorded 2026-09-24T03:23:26Z with Approval Evidence: Approve |
| A-2 | Complete task delivered | T-1 Complete and AC-1 through AC-7 Pass with actual evidence | Subtask and acceptance rows | Complete | T-1 Complete and AC-1 through AC-7 Pass; delivered through PR #19 (https://github.com/hailingu/koduck/pull/19), merged into `dev` as `9ba3039c5f1213ac70cf8e70505480420d472f1f` on 2026-09-24T06:53:44Z |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — repository verification governance, no ADD candidate selected | Architecture Source assessment | N/A — no ADD handoff | No product candidate status changes |
| A-4 | Requirement levels satisfied | Required proposal content complete and each conditional trigger assessed | Structured document review | Complete | Listed round-4 correction applied; required sections and trigger assessments retained; human approval recorded in A-1; implementation evidence is recorded in the subtask and acceptance rows above and through PR #19 |
| A-5 | Acceptance checks are decidable | Each check names T-1, inputs, deterministic method, exact result, and evidence | Structured acceptance review | Complete | Round-4 correction supplies AC-7 metrics in the full admission line and defines combined-output capture; all seven checks are Pass with their evidence recorded in the rows above |
| A-6 | Engineering exceptions governed, when applicable | No unapproved exception or executable-unit hard-limit violation in changed units | Changed-unit measurements and scope review | Complete | Point-in-time AST measurement of the implementation tree: longest changed executable units are `gate.analyze` (55 lines, unchanged), `test_flow.FlowTests.run_pre_push` (55) and `test_failed_check_emits_no_line_and_corrected_retry_analyzes_fresh` (52), all under the 80-line hard limit; largest changed files are `test_flow.py` (363) and `test_gate.py` (314), below the 1,000-line test review threshold, and `gate.py` (225) below the 600-line production threshold; no exception requested or required |
| A-7 | Contracts and risks covered | PV-1 through PV-7 traced; applicable risk rows Pass; N/A rows cite actual scope review | Traceability, risk and acceptance evidence | Complete | PV-1 through PV-7 each trace to passing checks AC-1 through AC-7; the three applicable Risk Coverage Matrix rows are Pass with test evidence and both N/A rows cite the AC-6 structured review; the Key State And Invariant Matrix rows S-1 through S-7 all map to those passing checks with no remaining gap |
| A-8 | Governance validation passed | Independent validator exits 0 for this ADR and index; after a committed snapshot exists, add its tested SHA through the Supporting Notes evidence-only follow-up | npm test --prefix tools/governance-validator; npm run validate --prefix tools/governance-validator | Complete | Committed-snapshot validation at 2026-09-24T03:42:05.266569+00:00: commit fcfec6bbdffd2c95735bb1d293e1cd36bb7e1cfd, ADR blob 9446b671f327e11dcb1f051d004eb6683acbc862, index blob f9346197517420be537971541548794f9b5f8bc6; npm test --prefix tools/governance-validator: 208/208 passed; npm run validate --prefix tools/governance-validator: Governance validation passed. This subsequent evidence-only update cites the tested commit, not its own revision; approval and implementation status are unchanged. Completion-closeout validation on 2026-09-24: 208/208 tests and Governance validation passed on the closeout revisions `5f65e378b05c2703d2a25110863594cc025ac8df` and `69b65fa35230419e9b1ebdf1362141d0c70c056d` (each validation executed on the tree exactly committed as the cited revision); this evidence-only follow-up cites those tested commits rather than its own revision |

## Supporting Notes [Optional]

The owner is @linhai, self-declared earlier in this task. All reviews below are
agent reviews, not owner approval. Implementation is Complete: delivered
through PR #19 and merged into `dev` as
`9ba3039c5f1213ac70cf8e70505480420d472f1f`; no
lower product test count, shorter CI or elapsed-time saving is claimed.

Draft rounds 1 through 4 used base `6bf6a225f2c8641d894d1f0d17919bd46d6b064a`
and index blob `eddff63d1dd640aa959ff51778fc51fc61271745`.
Round 5 reviewed the pushed PR revision:

| Round | Reviewer and request | Reviewed ADR blob | Reported result |
| --- | --- | --- | --- |
| 1 | Codex structured draft review | 453bc616a714e10dadd3616c8494d99f742d7f94 | Reported no findings; later evidence-only edits produced the round-2 input |
| 2 | GitHub Copilot, at the user's request; report supplied in this task on 2026-09-24 | 275ba911d3e9f91a83e5240a09eca59a376fbd07 | Twelve findings; agent performed no edits or governance validation; corrections produced the round-3 input |
| 3 | User-requested agent read-only follow-up, as identified in the report supplied on 2026-09-24 | 8fe880c54c1dfde1ab892397eaaa518cc4abd287 | Two approval-sensitive findings plus five suggestions; no file changes by reviewer |
| 4 | User-authorized bounded read-only agent follow-up, as recorded in the current report supplied on 2026-09-24 | 9db842c3bb6f03a7feb7da1b17df70f8108fb874 | One required metric-evidence correction and two suggestions; prior findings confirmed fixed; reviewer made no file changes |
| 5 | Codex automatic review of PR #18, submitted 2026-09-24T05:27:21Z; reviewed commit `1b460bb49a16f6709ecc360c216d4bd054fa86c7`; owner supplied the review link in this task for correction | 267ee204f5f6c8dcaa656245d6988ab3c6db3519 | One P2 finding: Supporting Notes still described the committed-snapshot validation as future work after A-8 recorded it; corrected in this evidence-only update |

The earlier owner message authorized the bounded round-3 read-only follow-up.
The subsequent owner message authorized round 4 and directed its corrections
before human approval. After PR #18 was created, Codex automatic review covered
commit `1b460bb49a16f6709ecc360c216d4bd054fa86c7`. The owner supplied that
review link in this task for a bounded correction of its P2 finding. This note
records the observed review and response; it does not request another agent
review round.

Round-2 corrections supplied the missing policy anchors and markers, direct
Python CLI scope, non-duplicative counts, push dispatch case, deletion-only cost
disclosure and unused-reader removal. Round-3 corrections superseded the
exit-0/stored-record reporting rule and corrected the mistaken owner attribution.
Round-4 remediation appends issue/gate/coverage metrics to the same admission
line, documents combined-output capture, and defines the committed-evidence
follow-up below. No implementation source is changed by these draft corrections.

| Round-3 findings | Disposition |
| --- | --- |
| 1, 3 | Choose (a): expand and flush the post-require_pass line with tree/base/policy; recompute identities using existing helpers; qualify each target independently of aggregate exit; no persisted-record matching |
| 2 | Attribute round 2 to GitHub Copilot; record round-3 revision, agent provenance, bounded request and read-only result |
| 4, 5 | Enumerate preflight, SonarFixture and other in-process doubles; migrate shared caller-state assertions to revision_snapshot and remove only obsolete index behavior |
| 6, 7 | Record the Codex prefix basis below and rerun both governance commands with tested-blob evidence in A-8 |

Branch `codex/push-boundary-sonar` was created from local dev at the same base.
The Codex task environment specifies the `codex/` branch prefix; this uses
AGENTS.md's tool-prefix allowance. No fetch, commit or push had occurred at
the time of approval. The owner subsequently requested submission of the PR;
A-8 now records the first committed snapshot's governance validation.
The ADR became Accepted with Implementation Status `Not Started` following
@linhai's explicit approval in this task, recorded at 2026-09-24T03:23:26Z.
That status/evidence update changed no
approved decision content and did not itself start another agent review;
Implementation Status moved to `In Progress` only when T-1 implementation
began on the task branch.

The committed-snapshot follow-up is complete. A-8 records the tested commit
`fcfec6bbdffd2c95735bb1d293e1cd36bb7e1cfd`, its ADR/index blobs,
validation time and command results. The evidence-only update was committed as
`1b460bb49a16f6709ecc360c216d4bd054fa86c7`; A-8 cites the tested commit
rather than that later update. This is informational verification context, not
Approval Evidence or an approval-binding revision. Implementation acceptance
checks were `Not Started` at that time; their current evidence is recorded in
the acceptance rows above.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Inactive future-lifecycle guidance until the trigger applies. At that time,
record the canonical rejection or retirement metadata and truthful final status,
move this file under docs/adr/archive/ retaining its filename, update the index
and every live reference/marker, and preserve reciprocal replacement paths if
superseded. Verify no live record or marker cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-24 | Proposed push-boundary verification with removal of commit-only work, unchanged push/CI gates, and deterministic reduction criteria; implementation not started | @codex |
| 2026-09-24 | Applied round-2 findings to touchpoints, evidence eligibility, markers, CLI scope and non-duplicative acceptance; recorded review provenance and task branch; remains Proposed / Not Started | @codex |
| 2026-09-24 | Applied the bounded round-3 corrections: self-contained per-target admission output and identity procedure, corrected agent-review provenance, explicit test doubles and snapshot assertion migration, prefix basis and revision-bound validation; remains Proposed / Not Started | @codex |
| 2026-09-24 | Applied the bounded round-4 correction and suggestions: report issue/gate/coverage metrics in admission output, capture both Git output streams, record review authorization and specify tested-commit evidence follow-up; remains Proposed / Not Started | @codex |
| 2026-09-24 | Accepted after @linhai explicitly approved ADR-0017 in this task; approval recorded at 2026-09-24T03:23:26Z; implementation remains Not Started and approved decision content is unchanged | @linhai |
| 2026-09-24 | Corrected stale committed-snapshot follow-up wording identified by PR #18 Codex review of `1b460bb49a16f6709ecc360c216d4bd054fa86c7`; recorded round-5 review provenance without changing the approved decision | @codex |
| 2026-09-24 | Started T-1 implementation on branch `zcode/adr-0017-push-boundary-sonar` from `dev` at `988a920`: push-only activation, removed commit machinery, extended admission line, installer and snapshot migration, policy synchronization with markers; AC-1 through AC-5 Pass, AC-6/AC-7 pending committed-revision evidence; Implementation Status In Progress; approved decision content unchanged | @zcode |
| 2026-09-24 | Evidence-only follow-up at `bcaeb412850e4ee3ca872691ede46bc358662e09`: AC-6 Pass on the committed-revision diff and routed commands; AC-7 local admission gate passed via the canonical manual check with a matching read-only identity recomputation, after remediating the first attempt's incremental `python:S5778` finding and amending before any push; required CI and exact-revision review remain pending for review-ready status; approved decision content unchanged | @zcode |
| 2026-09-24 | Applied the P2 finding from Codex automatic review of PR #19 commit `778df7484c8a0f282ba2277d317cd7eb73fbf5ac`: synchronized status-dependent text (blocker metadata N/A reasons and Supporting Notes) with Implementation Status `In Progress`; approved decision content unchanged | @zcode |
| 2026-09-24 | Completion closeout after PR #19 merged into `dev` as `9ba3039c5f1213ac70cf8e70505480420d472f1f`: T-1 and AC-7 set Complete/Pass with merge, CI and review evidence; Implementation Status Complete; approved decision content unchanged | @zcode |
| 2026-09-24 | Applied the P2 finding from Codex automatic review of PR #20 commit `5f65e378b05c2703d2a25110863594cc025ac8df`: A-4 and A-5 checklist evidence cells no longer claim pending implementation evidence or checks, removing the contradiction with the Complete status; approved decision content unchanged | @zcode |
| 2026-09-24 | Applied the P2 finding from Codex automatic review of PR #20 commit `69b65fa35230419e9b1ebdf1362141d0c70c056d`: A-8's completion-closeout evidence now names the exact tested revisions `5f65e37` and `69b65fa` and their command results; approved decision content unchanged | @zcode |
