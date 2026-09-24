"""Verify local command routing at process boundaries."""

# ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md

import contextlib
import io
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

import gate
import test_gate


class EntrypointTests(unittest.TestCase):
    """Exercise actual Git identities while substituting only scan and database I/O."""

    def setUp(self):
        test_gate.SnapshotTests.setUp(self)

    def test_retired_pre_commit_mode_exits_2_before_gate_resources(self):
        # AC-1: direct Python CLI rejects the retired mode before any credentials
        # or analysis resources; no shell wrapper is involved.
        environment = {
            key: value
            for key, value in os.environ.items()
            if key not in {"KODUCK_SONAR_TOKEN", "SONAR_TOKEN"}
        }
        process = subprocess.run(
            [sys.executable, str(Path(gate.__file__).resolve()), "pre-commit"],
            cwd=self.root,
            env=environment,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(process.returncode, 2, process.stderr)
        self.assertFalse((self.root / ".git/sonarqube").exists())

    def test_push_and_check_record_the_requested_tree(self):
        previous = os.getcwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        revision = test_gate.git(self.root, "rev-parse", "HEAD")
        ref_update = (
            "refs/heads/feature " + revision + " refs/heads/feature " + "0" * 40
        )
        for arguments, stream in (
            (["pre-push"], io.StringIO(ref_update + "\n")),
            (["check", "--base", revision], None),
        ):

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

            with contextlib.ExitStack() as stack:
                stack.enter_context(patch("sys.argv", ["gate", *arguments]))
                if stream is not None:
                    stack.enter_context(patch("sys.stdin", stream))
                stack.enter_context(
                    patch.dict(os.environ, {"KODUCK_SONAR_TOKEN": "fixture"})
                )
                stack.enter_context(
                    patch.object(gate, "database_fixture", contextlib.nullcontext)
                )
                stack.enter_context(
                    patch.object(gate, "project_lock", contextlib.nullcontext)
                )
                stack.enter_context(patch.object(gate, "analyze", side_effect=analyze))
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                self.assertEqual(gate.main(), 0)
            folder = self.root / ".git/sonarqube"
            record = json.loads(next(folder.glob("*.json")).read_text())
            self.assertEqual(
                record["tree"], test_gate.git(self.root, "rev-parse", "HEAD^{tree}")
            )
