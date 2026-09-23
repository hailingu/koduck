"""Exercise Rust LCOV import and scanner report handling at process boundaries."""

import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch

import coverage_report
import scan_runtime
import test_gate
from git_snapshot import index_snapshot


class ReportTests(unittest.TestCase):
    """Catch malformed LCOV and accidental tooling coverage imports."""

    def test_lcov_merges_hits_and_rejects_empty_or_foreign_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = root / "coverage.info"
            report.write_text("SF:src/lib.rs\nDA:2,0\nDA:2,1\nDA:3,0\nend_of_record\n")
            self.assertEqual(
                coverage_report.read_lcov(report, root),
                {"src/lib.rs": {2: True, 3: False}},
            )
            report.write_text("TN:empty\n")
            with self.assertRaisesRegex(RuntimeError, "COVERAGE_EMPTY"):
                coverage_report.read_lcov(report, root)
            report.write_text("SF:/outside/foreign.rs\nDA:2,1\n")
            with self.assertRaisesRegex(RuntimeError, "FOREIGN_PATH"):
                coverage_report.read_lcov(report, root)

    def test_only_product_rust_coverage_feeds_the_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = root / "reports"
            scope = scan_runtime.RustSourceScope(frozenset({"src/lib.rs"}), frozenset())
            with patch.object(
                scan_runtime,
                "rust_coverage",
                return_value=(
                    {
                        "src/lib.rs": {1: True},
                        "tools/sonarqube/gate.py": {1: True},
                    },
                    scope,
                ),
            ):
                result = scan_runtime.coverage(root, report, {"test_timeout": 2})
            self.assertEqual(result.hits, {"src/lib.rs": {1: True}})
            document = ET.parse(report / "coverage.xml")
            self.assertEqual(document.getroot().attrib, {"version": "1"})
            self.assertEqual(len(document.findall(".//lineToCover")), 1)


class ScanTests(unittest.TestCase):
    """Catch accepting a stale report or scanning a mutated source snapshot."""

    def setUp(self):
        test_gate.SnapshotTests.setUp(self)

    def test_production_scan_settles_its_new_task(self):
        class Server:
            host, token = "http://localhost:9000", "fixture-token"

            def wait(self, task, _timeout):
                if task != "fixture-task":
                    raise RuntimeError("wrong task")
                return "fixture-analysis"

        with (
            index_snapshot(self.root) as snapshot,
            tempfile.TemporaryDirectory() as directory,
        ):
            output = Path(directory) / "scan"

            def scanner(args, _cwd, _timeout, env):
                self.assertEqual(env["SONAR_TOKEN"], "fixture-token")
                self.assertNotIn("fixture-token", " ".join(args))
                self.assertIn(
                    "-Dsonar.coverage.exclusions=tools/**,scripts/**,.githooks/**",
                    args,
                )
                (output / "report-task.txt").write_text(
                    "projectKey=koduck\nceTaskId=fixture-task\n"
                )
                return ""

            config = {
                "project": "koduck",
                "exclusions": "",
                "tests": "",
                "compute_timeout": 1,
            }
            with patch.object(scan_runtime, "run", scanner):
                self.assertEqual(
                    scan_runtime.scan(snapshot, config, Server(), output),
                    ("fixture-task", "fixture-analysis"),
                )
            (snapshot.path / "code.py").write_text("changed\n")
            changed_output = Path(directory) / "changed"
            server = Server()
            with self.assertRaisesRegex(RuntimeError, "SNAPSHOT_CHANGED"):
                scan_runtime.scan(snapshot, config, server, changed_output)


if __name__ == "__main__":
    unittest.main()
