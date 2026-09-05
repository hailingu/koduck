"""Real subprocess and HTTP boundary tests for safe scan execution."""

import io
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

    def test_token_rejects_header_injection_before_network_access(self):
        module = implementation("sonar_api")
        with self.assertRaisesRegex(RuntimeError, "TOKEN_INVALID"):
            module.Sonar("http://localhost:9000", "koduck", "bad\nheader")


class EvidenceTests(unittest.TestCase):
    """Catch admitting stale analysis state or an altered cached result."""

    def test_evidence_lookup_is_bound_to_tree_base_and_policy(self):
        module = implementation("gate")
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            record = {"tree": "a", "base": "b", "policy": "c"}
            module.store_evidence(folder, record)
            self.assertEqual(module.load_evidence(folder, "a", "b", "c"), record)
            self.assertIsNone(module.load_evidence(folder, "a", "changed", "c"))


class RunnerTests(unittest.TestCase):
    """Catch leaking bootstrap credentials into runner CLI arguments."""

    def test_container_command_passes_no_secret_names_or_values(self):
        module = implementation("runner")
        with patch.dict(os.environ, {"KODUCK_SONAR_TOKEN": "fixture-private-token"}):
            command = module.container_command("fixture", "fixture-network")
        self.assertNotIn("fixture-private-token", " ".join(command))
        self.assertNotIn("KODUCK_SONAR_TOKEN", command)
        self.assertNotIn("KODUCK_AI_TEST_DATABASE_URL", command)
        self.assertNotIn("/var/run/docker.sock", " ".join(command))

    def test_runner_name_never_accepts_shell_metacharacters(self):
        module = implementation("runner")
        with self.assertRaisesRegex(RuntimeError, "RUNNER_NAME"):
            module.container_command("bad;touch /tmp/not-allowed", "network")


if __name__ == "__main__":
    unittest.main()
