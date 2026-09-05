"""Verify command routing and disposable runner lifecycle at process boundaries."""

import contextlib
import io
import json
import os
import unittest
from unittest.mock import patch

import gate
import runner
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


class RunnerLifecycleTests(unittest.TestCase):
    """Ensure failed workers restore readiness and release their owned resources."""

    def test_success_and_failure_always_disable_runner_and_cleanup(self):
        for fail in (False, True):
            calls = []

            def command(args, data=None, timeout=120, extra=None):
                calls.append((args, data, extra))
                if "generate-jitconfig" in " ".join(args):
                    return '{"encoded_jit_config":"fixture-jit"}'
                if args[:3] == ["docker", "run", "--rm"] and fail:
                    raise RuntimeError("fixture failure")
                return ""

            with (
                patch.object(runner, "command", side_effect=command),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                if fail:
                    with self.assertRaisesRegex(RuntimeError, "fixture failure"):
                        runner.launch("fixture", "fixture-token")
                else:
                    runner.launch("fixture", "fixture-token")
            self.assertEqual(calls[-1][0][-1], "false")
            self.assertTrue(
                any(
                    args == ["docker", "network", "rm", "fixture-network"]
                    for args, _, _ in calls
                )
            )
            for args, _, _ in calls:
                self.assertNotIn("fixture-token", args)
                self.assertNotIn("fixture-jit", args)
            worker = next(
                call for call in calls if call[0][:3] == ["docker", "run", "--rm"]
            )
            self.assertEqual(worker[1], "fixture-jit\n")
            self.assertEqual(worker[2]["KODUCK_SONAR_TOKEN"], "fixture-token")
