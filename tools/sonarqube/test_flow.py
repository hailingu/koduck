"""Exercise scan orchestration with real Git and doubles only at expensive I/O."""

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import gate
from git_snapshot import index_snapshot
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


class FlowTests(unittest.TestCase):
    """Catch incorrect base/target comparison and cache-based push bypasses."""

    def setUp(self):
        test_gate.SnapshotTests.setUp(self)
        (self.root / "code.py").write_text("value = 2\n")
        git(self.root, "add", "code.py")
        self.base = git(self.root, "rev-parse", "HEAD")

    def test_real_snapshot_analysis_result_admits_only_zero_findings(self):
        for issue in (False, True):
            with index_snapshot(self.root) as snapshot:
                with (
                    patch.object(gate, "preflight"),
                    patch.object(gate, "coverage", return_value={"code.py": {1: True}}),
                    patch.object(gate, "scan", return_value=("task", "analysis")),
                ):
                    record = gate.analyze(
                        self.root, snapshot, self.base, {}, SonarFixture(issue)
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

    def test_cached_result_never_skips_fresh_push_scan(self):
        revision = git(self.root, "rev-parse", "HEAD")
        tree = git(self.root, "rev-parse", "HEAD^{tree}")
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            record = {
                "tree": tree,
                "base": revision,
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
                gate.check_revision(
                    self.root, revision, folder, {}, SonarFixture(), revision
                )
                analyze.assert_called_once()

    def test_missing_cache_is_analyzed_and_persisted_before_push(self):
        revision = git(self.root, "rev-parse", "HEAD")
        tree = git(self.root, "rev-parse", "HEAD^{tree}")
        record = {
            "tree": tree,
            "base": revision,
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
            gate.check_revision(
                self.root, revision, folder, {}, SonarFixture(), revision
            )
            self.assertEqual(
                gate.load_evidence(folder, tree, revision, gate.policy_id()), record
            )

    def test_invalid_cache_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            gate.evidence_path(folder, "a", "b", "c").write_text("not json")
            with self.assertRaisesRegex(RuntimeError, "EVIDENCE_INVALID"):
                gate.load_evidence(folder, "a", "b", "c")
