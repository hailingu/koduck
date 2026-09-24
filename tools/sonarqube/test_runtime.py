"""Real subprocess and HTTP boundary tests for safe scan execution."""

# ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md

import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

from test_gate import implementation


class RuntimeTests(unittest.TestCase):
    """Detect token leakage, fabricated success and missing report identities."""

    def test_failed_process_does_not_echo_secret_output(self):
        module = implementation("scan_runtime")
        with tempfile.TemporaryDirectory() as directory:
            capture = io.StringIO()
            with (
                redirect_stdout(capture),
                self.assertRaisesRegex(RuntimeError, "COMMAND_FAILED"),
            ):
                module.run(
                    ["python3", "-c", "print('sentinel-private-content'); exit(3)"],
                    Path(directory),
                    seconds=5,
                )
            self.assertNotIn("sentinel-private-content", capture.getvalue())

    def test_test_process_does_not_inherit_analysis_token(self):
        module = implementation("scan_runtime")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "result"
            code = "import os,pathlib; pathlib.Path('result').write_text(str('SONAR_TOKEN' in os.environ or 'KODUCK_SONAR_TOKEN' in os.environ))"
            with patch.dict(
                os.environ, {"SONAR_TOKEN": "fixture", "KODUCK_SONAR_TOKEN": "fixture"}
            ):
                module.run(["python3", "-c", code], Path(directory), seconds=5)
            self.assertEqual(output.read_text(), "False")

    def test_report_requires_correct_project_and_task(self):
        module = implementation("scan_runtime")
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report-task.txt"
            report.write_text("projectKey=koduck\nceTaskId=fixture-id\n")
            self.assertEqual(module.task_id(report, "koduck"), "fixture-id")
            report.write_text("projectKey=foreign\nceTaskId=fixture-id\n")
            with self.assertRaises(RuntimeError):
                module.task_id(report, "koduck")

    def test_rust_coverage_runs_the_integration_report_script(self):
        """The gate must import the report created by its Shell Rust boundary."""
        module = implementation("scan_runtime")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            output.mkdir()
            commands = []

            def process(command, _cwd, _seconds):
                commands.append(command)
                if command[0] == "sh":
                    Path(command[-1]).write_text(
                        "SF:src/lib.rs\nDA:1,1\nend_of_record\n"
                    )
                return ""

            scope = module.RustSourceScope(frozenset({"src/lib.rs"}), frozenset())
            with (
                patch.object(module, "run", process),
                patch.object(module, "compiled_rust_scope", return_value=scope),
            ):
                hits, actual_scope = module.rust_coverage(root, output, 2)
            self.assertEqual(hits, {"src/lib.rs": {1: True}})
            self.assertEqual(actual_scope, scope)
            self.assertEqual(commands[-1][0], "sh")

    def test_preflight_needs_no_tooling_coverage_installation(self):
        """A Rust-only gate should run without the former c8/parser environment."""
        module = implementation("scan_runtime")
        config = {"scanner_version": "7.3", "llvm_cov_version": "0.9"}
        with (
            tempfile.TemporaryDirectory() as directory,
            patch.dict(os.environ, {"KODUCK_AI_TEST_DATABASE_URL": "fixture"}),
            patch.object(module, "run", side_effect=["7.3", "0.9"]),
        ):
            module.preflight(config, Path(directory))

    def test_token_rejects_header_injection_before_network_access(self):
        module = implementation("sonar_api")
        with self.assertRaisesRegex(RuntimeError, "TOKEN_INVALID"):
            module.Sonar("http://localhost:9000", "koduck", "bad\nheader")


class EvidenceTests(unittest.TestCase):
    """Catch admitting stale analysis state through a mismatched identity key."""

    def test_evidence_is_persisted_under_its_identity_key(self):
        module = implementation("gate")
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            first = {"tree": "a", "base": "b", "policy": "c"}
            second = {"tree": "a", "base": "changed", "policy": "c"}
            module.store_evidence(folder, first)
            module.store_evidence(folder, second)
            path_first = module.evidence_path(folder, "a", "b", "c")
            path_second = module.evidence_path(folder, "a", "changed", "c")
            self.assertNotEqual(path_first, path_second)
            self.assertEqual(json.loads(path_first.read_text()), first)
            self.assertEqual(json.loads(path_second.read_text()), second)
            self.assertFalse(module.evidence_path(folder, "other", "b", "c").exists())


if __name__ == "__main__":
    unittest.main()
