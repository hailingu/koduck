"""Verify local command routing at process boundaries."""

import contextlib
import io
import json
import os
import unittest
from unittest.mock import patch

import gate
import test_gate


class EntrypointTests(unittest.TestCase):
    """Exercise actual Git identities while substituting only scan and database I/O."""

    def setUp(self):
        test_gate.SnapshotTests.setUp(self)

    def test_commit_and_check_record_the_requested_tree(self):
        previous = os.getcwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        revision = test_gate.git(self.root, "rev-parse", "HEAD")
        for arguments in (["pre-commit"], ["check", "--base", revision]):

            def analyze(_root, snapshot, base, _config, _sonar):
                return {
                    "tree": snapshot.tree,
                    "base": base,
                    "policy": gate.policy_id(),
                    "analysis": "fixture",
                    "quality_gate": "OK",
                    "new_issues": 0,
                    "covered": 0,
                    "coverable": 0,
                }

            with (
                patch("sys.argv", ["gate", *arguments]),
                patch.dict(os.environ, {"KODUCK_SONAR_TOKEN": "fixture"}),
                patch.object(gate, "database_fixture", contextlib.nullcontext),
                patch.object(gate, "project_lock", contextlib.nullcontext),
                patch.object(gate, "analyze", side_effect=analyze),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(gate.main(), 0)
            folder = self.root / ".git/sonarqube"
            record = json.loads(next(folder.glob("*.json")).read_text())
            self.assertEqual(record["tree"], test_gate.git(self.root, "write-tree"))
