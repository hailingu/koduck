"""Exercise coverage generation and scanner report handling at process boundaries."""

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
    """Catch stale/malformed reports and inconsistent paths across coverage producers."""

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

    def test_all_producers_feed_the_same_generic_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tools/sonarqube").mkdir(parents=True)
            (root / "tools/sonarqube/test_gate.py").write_text("# fixture\n")
            (root / "tools/sonarqube/gate.py").write_text("value = 1\n")

            def process(args, *_unused, **_kwargs):
                if "--output-path" in args:
                    report = Path(args[args.index("--output-path") + 1])
                    report.write_text("SF:src/lib.rs\nDA:1,1\nend_of_record\n")
                if "--reports-dir" in args:
                    output = Path(args[args.index("--reports-dir") + 1])
                    output.mkdir()
                    (output / "lcov.info").write_text(
                        "SF:tools/governance-validator/validate.mjs\nDA:1,1\nend_of_record\n"
                    )
                if "xml" in args:
                    report = Path(args[args.index("-o") + 1])
                    report.write_text(
                        '<coverage><packages><package><classes><class filename="tools/sonarqube/gate.py"><lines><line number="1" hits="1"/></lines></class></classes></package></packages></coverage>'
                    )
                return ""

            with patch.object(scan_runtime, "run", process):
                result = scan_runtime.coverage(
                    root, root, root / "reports", {"test_timeout": 2}
                )
            self.assertEqual(
                set(result),
                {
                    "src/lib.rs",
                    "tools/governance-validator/validate.mjs",
                    "tools/sonarqube/gate.py",
                },
            )
            document = ET.parse(root / "reports/coverage.xml")
            self.assertEqual(document.getroot().attrib, {"version": "1"})
            self.assertEqual(
                len(document.findall(".//lineToCover[@covered='true']")), 3
            )


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

            def require_current(self, task):
                if task != "fixture-task":
                    raise RuntimeError("wrong task")

        with (
            index_snapshot(self.root) as snapshot,
            tempfile.TemporaryDirectory() as directory,
        ):
            output = Path(directory) / "scan"

            def scanner(args, _cwd, _timeout, env):
                self.assertEqual(env["SONAR_TOKEN"], "fixture-token")
                self.assertNotIn("fixture-token", " ".join(args))
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
            server, changed = Server(), Path(directory) / "changed"
            with self.assertRaisesRegex(RuntimeError, "SNAPSHOT_CHANGED"):
                scan_runtime.scan(snapshot, config, server, changed)
