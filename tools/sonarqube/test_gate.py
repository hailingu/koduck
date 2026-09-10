"""Behavioral regression checks for immutable Git and Sonar push admission."""

import importlib
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


def implementation(name):
    """Make a missing production boundary an explicit red-phase failure."""
    try:
        return importlib.import_module(name)
    except ModuleNotFoundError:
        raise AssertionError(f"Missing production boundary: {name}") from None


def git(root, *args):
    """Run real Git against a disposable fixture repository."""
    return (
        subprocess.check_output(
            ["git", "-C", str(root), *args], stderr=subprocess.DEVNULL
        )
        .decode()
        .strip()
    )


class SnapshotTests(unittest.TestCase):
    """Catch scanning unstaged content and accepting a changed index."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        git(self.root, "init", "-b", "dev")
        git(self.root, "config", "user.name", "Fixture")
        git(self.root, "config", "user.email", "fixture@example.invalid")
        (self.root / "code.py").write_text("value = 1\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-m", "baseline")

    def test_snapshot_contains_index_only_and_preserves_worktree(self):
        module = implementation("git_snapshot")
        (self.root / "code.py").write_text("value = 2\n")
        git(self.root, "add", "code.py")
        (self.root / "code.py").write_text("value = 3\n")
        (self.root / "untracked.py").write_text("secret = 'fixture'\n")
        before = git(self.root, "status", "--porcelain")
        with module.index_snapshot(self.root) as snapshot:
            self.assertEqual((snapshot.path / "code.py").read_text(), "value = 2\n")
            self.assertFalse((snapshot.path / "untracked.py").exists())
            self.assertEqual(git(snapshot.path, "status", "--porcelain"), "")
            self.assertEqual(snapshot.tree, git(self.root, "write-tree"))
        self.assertEqual(git(self.root, "status", "--porcelain"), before)
        self.assertEqual((self.root / "code.py").read_text(), "value = 3\n")

    def test_index_change_invalidates_completed_scan(self):
        module = implementation("git_snapshot")
        with module.index_snapshot(self.root) as snapshot:
            (self.root / "code.py").write_text("value = 9\n")
            git(self.root, "add", "code.py")
            with self.assertRaisesRegex(RuntimeError, "INDEX_CHANGED"):
                module.require_index(self.root, snapshot.tree)

    def test_push_checks_proposed_object_not_current_head(self):
        module = implementation("git_snapshot")
        revision = git(self.root, "rev-parse", "HEAD")
        rows = f"refs/heads/feature {revision} refs/heads/feature {'0' * 40}\n"
        self.assertEqual(module.push_revisions(self.root, rows), [revision])
        self.assertEqual(
            module.push_revisions(self.root, rows.replace(revision, "0" * 40)), []
        )
        with self.assertRaisesRegex(RuntimeError, "PUSH_INPUT"):
            module.push_revisions(self.root, "malformed\n")


class AdmissionTests(unittest.TestCase):
    """Catch false green results, stale evidence and lost duplicate issues."""

    def test_new_issue_multiset_does_not_hide_a_duplicate(self):
        module = implementation("sonar_api")
        old = {
            "rule": "rust:S1",
            "component": "koduck:a.rs",
            "hash": "abc",
            "message": "x",
        }
        self.assertEqual(module.incremental_issues([old], [old]), 0)
        self.assertEqual(module.incremental_issues([old], [old, old]), 1)
        self.assertEqual(
            module.incremental_issues([old], [{**old, "rule": "rust:S2"}]), 1
        )

    def test_failure_missing_metrics_and_stale_binding_block_push(self):
        module = implementation("sonar_api")
        good = {
            "tree": "a",
            "base": "b",
            "policy": "c",
            "analysis": "id",
            "quality_gate": "OK",
            "new_issues": 0,
            "covered": 8,
            "coverable": 10,
        }
        module.require_pass(good, "a", "b", "c")
        for field, value in [
            ("tree", "wrong"),
            ("base", "wrong"),
            ("policy", "wrong"),
            ("quality_gate", "ERROR"),
            ("new_issues", 1),
            ("covered", 7),
            ("analysis", None),
        ]:
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                module.require_pass({**good, field: value}, "a", "b", "c")
        with self.assertRaises(RuntimeError):
            module.require_pass({}, "a", "b", "c")

    def test_zero_coverable_lines_is_not_missing_report(self):
        module = implementation("coverage_report")
        changed = {"src/lib.rs": {2, 3, 4}}
        self.assertEqual(
            module.changed_coverage(changed, {"src/lib.rs": {2: True, 3: False}}),
            (1, 2),
        )
        self.assertEqual(
            module.changed_coverage(changed, {"src/lib.rs": {1: True}}), (0, 0)
        )
        with self.assertRaisesRegex(RuntimeError, "COVERAGE_MISSING"):
            module.changed_coverage(changed, {})
        self.assertEqual(module.changed_coverage(changed, {}, {"src/lib.rs"}), (0, 0))

    def test_changed_shell_lines_stay_in_the_denominator_as_uncovered(self):
        module = implementation("coverage_report")
        changed = {".githooks/pre-push": {5, 6, 7}}
        self.assertEqual(
            module.changed_coverage(changed, {}),
            (0, 3),
            "a script-only change must produce coverable lines, never zero",
        )
        mixed = {**changed, "src/lib.rs": {2}}
        self.assertEqual(
            module.changed_coverage(mixed, {"src/lib.rs": {2: True}}), (1, 4)
        )
        snapshot = implementation("git_snapshot")
        self.assertTrue(snapshot.is_production_source("scripts/sonar-quality-gate.sh"))
        self.assertTrue(snapshot.is_production_source("tools/sonarqube/install.sh"))
        self.assertFalse(snapshot.is_production_source("README.md"))

    def test_runner_files_restore_credentials_without_job_environment(self):
        module = implementation("gate")
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / ".koduck").mkdir()
            (home / ".koduck" / "database-url").write_text("fixture-url\n")
            (home / ".koduck" / "sonar-token").write_text("fixture-token\n")
            with patch.dict(os.environ, {}, clear=False):
                os.environ.pop("KODUCK_AI_TEST_DATABASE_URL", None)
                with patch.object(module, "RUNNER_FILE_DIR", home / ".koduck"):
                    self.assertEqual(module.sonar_token(), "fixture-token")
                    module.restore_runner_database_url()
                    self.assertEqual(
                        os.environ["KODUCK_AI_TEST_DATABASE_URL"], "fixture-url"
                    )

    def test_script_only_changes_cannot_satisfy_the_coverage_gate(self):
        module = implementation("sonar_api")
        record = {
            "tree": "t",
            "base": "b",
            "policy": "p",
            "analysis": "a",
            "quality_gate": "OK",
            "new_issues": 0,
            "covered": 0,
            "coverable": 6,
        }
        with self.assertRaisesRegex(RuntimeError, "COVERAGE_BELOW_80"):
            module.require_pass(record, "t", "b", "p")

    def test_python_report_resolves_its_source_root(self):
        module = implementation("coverage_report")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "tools/sonarqube"
            source.mkdir(parents=True)
            (source / "gate.py").write_text("pass\n")
            report = root / "python.xml"
            report.write_text(
                f'<coverage><sources><source>{source}</source></sources><packages><package><classes><class filename="gate.py"><lines><line number="1" hits="1"/></lines></class></classes></package></packages></coverage>'
            )
            self.assertEqual(
                module.read_python(report, root), {"tools/sonarqube/gate.py": {1: True}}
            )


if __name__ == "__main__":
    unittest.main()


class DeclarationTests(unittest.TestCase):
    """Distinguish a conservative Rust declaration grammar from executable code."""

    def test_only_module_and_import_declarations_are_nonexecutable(self):
        module = implementation("coverage_report")
        self.assertTrue(
            module.rust_declarations_only(
                "// documentation\nmod child;\n#[cfg(test)]\npub(crate) use child::{Foo, Bar};\n"
            )
        )
        for source in (
            "fn run() {}",
            "mod child { fn run() {} }",
            'include!("code.rs");',
            "const X: i32 = compute();",
            "pub use child::Foo; fn run() {}",
        ):
            self.assertFalse(module.rust_declarations_only(source))
