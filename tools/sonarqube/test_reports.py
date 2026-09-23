"""Exercise coverage generation and scanner report handling at process boundaries."""

import json
import os
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest.mock import patch

import coverage_report
import scan_runtime
import shell_coverage
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

    def test_trace_probe_keeps_user_site_with_isolated_fixture_home(self):
        """CI user-site parser packages remain available to the trusted probe."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture_home = root / "fixture-home"
            user_base = root / "installed-user-packages"
            with patch.object(
                shell_coverage.site, "getuserbase", return_value=str(user_base)
            ):
                environment = shell_coverage.trace_environment(
                    root,
                    {"HOME": str(fixture_home), "PATH": os.environ["PATH"]},
                    Path(sys.executable),
                )
            self.assertEqual(environment["HOME"], str(fixture_home))
            self.assertEqual(environment["PYTHONUSERBASE"], str(user_base))

    def test_candidate_python_test_cannot_forge_gate_shell_hits(self):
        """A candidate test can write output files but cannot award Shell hits."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "source"
            root.mkdir()
            test_gate.git(root, "init", "-q")
            script = root / "uncovered.sh"
            script.write_text('printf "not executed\\n"\n')
            test_gate.git(root, "add", "uncovered.sh")
            output = Path(directory) / "output"
            output.mkdir()
            site = shell_coverage.command_sites(script.read_bytes())[0]
            forged = {
                "sources": {"uncovered.sh": shell_coverage.digest(script.read_bytes())},
                "hits": [["uncovered.sh", site[0], site[2], site[3]]],
            }

            def candidate_process(args, _cwd, _seconds, extra=None):
                if "unittest" in args:
                    self.assertNotIn("KODUCK_SHELL_COVERAGE", extra)
                    visible_output = Path(extra["COVERAGE_FILE"]).parent
                    reports = visible_output / "shell-traces"
                    reports.mkdir(exist_ok=True)
                    (reports / "forged.json").write_text(json.dumps(forged))
                elif args[-1].endswith("shell.lcov"):
                    hits = shell_coverage.collect(Path(args[-3]), Path(args[-2]))
                    Path(args[-1]).write_text(
                        "\n".join(
                            ["SF:uncovered.sh"]
                            + [
                                f"DA:{line},{int(hit)}"
                                for line, hit in hits["uncovered.sh"].items()
                            ]
                            + ["end_of_record"]
                        )
                    )
                return ""

            with (
                patch.object(scan_runtime, "run", candidate_process),
                patch.object(scan_runtime, "read_python", return_value={}),
                patch.object(scan_runtime, "verify_shell_entrypoints"),
            ):
                result = scan_runtime.python_coverage(root, root, output, 2)
            self.assertEqual(result["uncovered.sh"], {1: False})

    def test_trusted_shell_verification_executes_versioned_entrypoints(self):
        """Scanner-owned fixture runs produce source-bound Shell evidence."""
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as directory:
            reports = Path(directory)
            probe_python = Path(sys.executable)
            with patch.object(
                shell_coverage.sys, "executable", str(reports / "missing")
            ):
                shell_coverage.verify_shell_entrypoints(root, reports, probe_python)
            hits = shell_coverage.collect(root, reports)
        for name in (
            ".githooks/pre-commit",
            ".githooks/pre-push",
            "scripts/sonar-quality-gate.sh",
            "tools/sonarqube/install.sh",
        ):
            self.assertTrue(any(hits[name].values()), name)

    def test_failed_candidate_tests_never_start_trusted_shell_verification(self):
        """Failed candidate tests cannot leave importable gate Shell evidence."""
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            output.mkdir()
            with (
                patch.object(
                    scan_runtime, "run", side_effect=RuntimeError("test failed")
                ),
                patch.object(scan_runtime, "verify_shell_entrypoints") as verify,
            ):
                with self.assertRaisesRegex(RuntimeError, "test failed"):
                    scan_runtime.python_coverage(
                        Path(directory), Path(directory), output, 2
                    )
            verify.assert_not_called()
            self.assertFalse((output / "shell.lcov").exists())

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
                if args[-1].endswith("shell.lcov"):
                    Path(args[-1]).write_text(
                        "SF:.githooks/pre-push\nDA:2,1\nend_of_record\n"
                    )
                return ""

            with (
                patch.object(scan_runtime, "run", process),
                patch.object(scan_runtime, "verify_shell_entrypoints"),
            ):
                result = scan_runtime.coverage(
                    root, root, root / "reports", {"test_timeout": 2}
                )
            self.assertEqual(
                set(result),
                {
                    "src/lib.rs",
                    "tools/governance-validator/validate.mjs",
                    "tools/sonarqube/gate.py",
                    ".githooks/pre-push",
                },
            )
            document = ET.parse(root / "reports/coverage.xml")
            self.assertEqual(document.getroot().attrib, {"version": "1"})
            self.assertEqual(
                len(document.findall(".//lineToCover[@covered='true']")), 4
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
