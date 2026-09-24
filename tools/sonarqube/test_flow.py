"""Exercise scan orchestration with real Git and doubles only at expensive I/O."""

# ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md

import contextlib
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import gate
from git_snapshot import RustSourceScope, revision_snapshot
from scan_runtime import CoverageResult
from sonar_api import require_pass
import test_gate
from test_gate import git


class SonarFixture:
    """Model an unchanged baseline and a candidate with a selectable new finding."""

    def __init__(self, issue=False):
        self.reads = 0
        self.issue = issue

    def findings(self):
        self.reads += 1
        if self.issue and self.reads == 2:
            return [
                {
                    "rule": "python:S1",
                    "component": "koduck:code.py",
                    "message": "fixture",
                }
            ]
        return []

    def gate(self, _analysis):
        return "OK"

    def nonexecutable_files(self, _names):
        return set()


class SequenceSonar:
    """Admit the first pushed target and report a new finding for the second."""

    def __init__(self):
        self.reads = 0

    def findings(self):
        self.reads += 1
        if self.reads == 4:
            return [
                {
                    "rule": "python:S1",
                    "component": "koduck:code.py",
                    "message": "fixture",
                }
            ]
        return []

    def gate(self, _analysis):
        return "OK"

    def nonexecutable_files(self, _names):
        return set()


class FlowTests(unittest.TestCase):
    """Catch incorrect base/target comparison and cache-based push bypasses."""

    def setUp(self):
        test_gate.SnapshotTests.setUp(self)
        self.base = git(self.root, "rev-parse", "HEAD")
        git(self.root, "checkout", "-b", "feature")
        (self.root / "code.py").write_text("value = 2\n")
        git(self.root, "add", "code.py")
        git(self.root, "commit", "-m", "feature")
        self.revision = git(self.root, "rev-parse", "HEAD")

    def run_pre_push(self, updates, sonar=None):
        """Drive the real pre-push dispatch in process with counted expensive doubles."""
        counts = {"coverage": 0, "scans": 0, "fixture": 0}

        def coverage(_snapshot, _output, _config):
            counts["coverage"] += 1
            return CoverageResult(
                {"code.py": {1: True}}, RustSourceScope(frozenset(), frozenset())
            )

        def scan(_snapshot, _config, _sonar, _output, _report=None):
            counts["scans"] += 1
            return ("task", "analysis-" + str(counts["scans"]))

        @contextlib.contextmanager
        def fixture():
            counts["fixture"] += 1
            yield

        stdin = io.StringIO(
            "".join(
                name + " " + revision + " " + name + " " + "0" * 40 + "\n"
                for name, revision in updates
            )
        )
        previous = os.getcwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        output = io.StringIO()
        outcome = {"stdout": "", "counts": counts}
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch("sys.argv", ["gate", "pre-push"]))
            stack.enter_context(patch("sys.stdin", stdin))
            stack.enter_context(
                patch.dict(os.environ, {"KODUCK_SONAR_TOKEN": "fixture"})
            )
            stack.enter_context(
                patch.object(gate, "Sonar", lambda *_arguments: sonar or SonarFixture())
            )
            stack.enter_context(
                patch.object(gate, "preflight", lambda *_arguments: None)
            )
            stack.enter_context(patch.object(gate, "coverage", coverage))
            stack.enter_context(patch.object(gate, "scan", scan))
            stack.enter_context(patch.object(gate, "database_fixture", fixture))
            stack.enter_context(
                patch.object(gate, "project_lock", contextlib.nullcontext)
            )
            stack.enter_context(contextlib.redirect_stdout(output))
            try:
                outcome["status"] = gate.main()
            except RuntimeError as error:
                outcome["error"] = error
        outcome["stdout"] = output.getvalue()
        return outcome

    def admission_lines(self, stdout):
        return [
            line
            for line in stdout.splitlines()
            if line.startswith("Sonar push admitted:")
        ]

    def test_single_target_push_runs_one_coverage_two_scans_one_fixture(self):
        """AC-2/PV-7 basis: one pushed target schedules exactly one analysis workload."""
        outcome = self.run_pre_push([("refs/heads/feature", self.revision)])
        self.assertNotIn("error", outcome)
        self.assertEqual(outcome["status"], 0)
        self.assertEqual(outcome["counts"], {"coverage": 1, "scans": 2, "fixture": 1})

    def test_pre_push_emits_one_complete_admission_line_for_admitted_target(self):
        tree = git(self.root, "rev-parse", self.revision + "^{tree}")
        outcome = self.run_pre_push([("refs/heads/feature", self.revision)])
        lines = self.admission_lines(outcome["stdout"])
        self.assertEqual(len(lines), 1)
        self.assertEqual(
            lines[0],
            "Sonar push admitted: "
            + self.revision
            + " analysis=analysis-2"
            + " tree="
            + tree
            + " base="
            + self.base
            + " policy="
            + gate.policy_id()
            + " new_issues=0 quality_gate=OK covered=1 coverable=1",
        )

    def test_failed_later_target_never_retracts_the_earlier_admission_line(self):
        (self.root / "code.py").write_text("value = 3\n")
        git(self.root, "add", "code.py")
        git(self.root, "commit", "-m", "second")
        second = git(self.root, "rev-parse", "HEAD")
        outcome = self.run_pre_push(
            [("refs/heads/feature", self.revision), ("refs/heads/other", second)],
            sonar=SequenceSonar(),
        )
        self.assertIn("error", outcome)
        lines = self.admission_lines(outcome["stdout"])
        self.assertEqual(len(lines), 1)
        self.assertTrue(
            lines[0].startswith("Sonar push admitted: " + self.revision + " "),
            lines[0],
        )

    def test_failed_check_emits_no_line_and_corrected_retry_analyzes_fresh(self):
        record = {
            "tree": git(self.root, "rev-parse", self.revision + "^{tree}"),
            "base": self.revision,
            "policy": gate.policy_id(),
            "analysis": "analysis",
            "quality_gate": "OK",
            "new_issues": 0,
            "covered": 0,
            "coverable": 0,
        }
        for override in (
            {"new_issues": 1},
            {"quality_gate": "ERROR"},
            {"covered": 7, "coverable": 10},
            {"tree": "wrong"},
        ):
            with self.subTest(override=override):
                calls = []
                stdout = io.StringIO()
                failing = {**record, **override}

                def analyze(_root, _snapshot, _base, _config, _sonar):
                    calls.append(1)
                    return failing if len(calls) == 1 else record

                sonar = SonarFixture()
                with (
                    tempfile.TemporaryDirectory() as directory,
                    patch.object(gate, "analyze", side_effect=analyze),
                    contextlib.redirect_stdout(stdout),
                ):
                    folder = Path(directory)
                    with self.assertRaises(RuntimeError):
                        gate.check_revision(
                            self.root, self.revision, folder, {}, sonar, self.revision
                        )
                    self.assertTrue(any(folder.glob("*.json")))
                    gate.check_revision(
                        self.root, self.revision, folder, {}, sonar, self.revision
                    )
                self.assertEqual(len(calls), 2)
                self.assertEqual(stdout.getvalue().count("Sonar push admitted:"), 1)

    def test_real_snapshot_analysis_result_admits_only_zero_findings(self):
        for issue in (False, True):
            with revision_snapshot(self.root, self.revision) as snapshot:
                with (
                    patch.object(gate, "preflight"),
                    patch.object(
                        gate,
                        "coverage",
                        return_value=CoverageResult(
                            {"code.py": {1: True}},
                            RustSourceScope(frozenset(), frozenset()),
                        ),
                    ),
                    patch.object(gate, "scan", return_value=("task", "analysis")),
                ):
                    record = gate.analyze(
                        self.root,
                        snapshot,
                        self.base,
                        {"tests": "**/tests/**"},
                        SonarFixture(issue),
                    )
                self.assertEqual(record["tree"], snapshot.tree)
                self.assertEqual(record["base"], self.base)
                self.assertEqual((record["covered"], record["coverable"]), (1, 1))
                self.assertEqual(record["new_issues"], int(issue))
                if issue:
                    with self.assertRaisesRegex(RuntimeError, "INCREMENTAL_FINDINGS"):
                        require_pass(record, snapshot.tree, self.base, record["policy"])
                else:
                    require_pass(record, snapshot.tree, self.base, record["policy"])

    def test_changed_production_rust_under_scanner_test_path_blocks(self):
        """Reject a compiled source the static Sonar test mapping would hide."""
        name = "src/feature/tests/engine.rs"
        path = self.root / name
        path.parent.mkdir(parents=True)
        path.write_text("pub fn value() -> u64 { 1 }\n")
        git(self.root, "add", name)
        git(self.root, "commit", "-m", "engine")
        revision = git(self.root, "rev-parse", "HEAD")
        report = CoverageResult(
            {name: {1: False}}, RustSourceScope(frozenset({name}), frozenset())
        )
        with revision_snapshot(self.root, revision) as snapshot:
            sonar = SonarFixture()
            with (
                patch.object(gate, "preflight"),
                patch.object(gate, "coverage", return_value=report),
                patch.object(gate, "scan") as scan,
            ):
                with self.assertRaisesRegex(RuntimeError, "RUST_PRODUCTION_TEST_PATH"):
                    gate.analyze(
                        self.root,
                        snapshot,
                        self.base,
                        {"tests": "**/tests/**"},
                        sonar,
                    )
                scan.assert_not_called()

    def test_cached_result_never_skips_fresh_push_scan(self):
        tree = git(self.root, "rev-parse", self.revision + "^{tree}")
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            record = {
                "tree": tree,
                "base": self.revision,
                "policy": gate.policy_id(),
                "analysis": "analysis",
                "task": "task",
                "quality_gate": "OK",
                "new_issues": 1,
                "covered": 1,
                "coverable": 1,
            }
            gate.store_evidence(folder, record)
            record["new_issues"] = 0
            with patch.object(gate, "analyze", return_value=record) as analyze:
                stdout = io.StringIO()
                with contextlib.redirect_stdout(stdout):
                    gate.check_revision(
                        self.root,
                        self.revision,
                        folder,
                        {},
                        SonarFixture(),
                        self.revision,
                    )
                analyze.assert_called_once()
            self.assertEqual(len(self.admission_lines(stdout.getvalue())), 1)

    def test_missing_cache_is_analyzed_and_persisted_before_push(self):
        tree = git(self.root, "rev-parse", self.revision + "^{tree}")
        record = {
            "tree": tree,
            "base": self.revision,
            "policy": gate.policy_id(),
            "analysis": "analysis",
            "task": "task",
            "quality_gate": "OK",
            "new_issues": 0,
            "covered": 0,
            "coverable": 0,
        }
        with (
            tempfile.TemporaryDirectory() as directory,
            patch.object(gate, "analyze", return_value=record),
        ):
            folder = Path(directory)
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                gate.check_revision(
                    self.root, self.revision, folder, {}, SonarFixture(), self.revision
                )
            stored = gate.evidence_path(
                folder, record["tree"], record["base"], record["policy"]
            )
            self.assertEqual(json.loads(stored.read_text()), record)
            self.assertEqual(len(self.admission_lines(stdout.getvalue())), 1)
