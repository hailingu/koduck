"""Behavioral regression checks for immutable Git and Sonar push admission."""

import importlib
import json
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

    def test_rust_test_modules_do_not_count_as_changed_production(self):
        """Count a production sibling but exclude a dedicated Rust test path."""
        module = implementation("git_snapshot")
        base = git(self.root, "rev-parse", "HEAD")
        owner = self.root / "src/feature.rs"
        owner.parent.mkdir()
        owner.write_text(
            '#[cfg(test)]\n#[path = "feature/tests/case.rs"]\n'
            "mod case;\n\npub fn live() -> u64 { 1 }\n"
        )
        test_module = self.root / "src/feature/tests/case.rs"
        test_module.parent.mkdir(parents=True)
        test_module.write_text("#[test]\nfn fixture() { assert_eq!(2, 2); }\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-m", "add Rust module")
        scope = module.RustSourceScope(
            frozenset({"src/feature.rs"}),
            frozenset({"src/feature/tests/case.rs"}),
        )
        changed = module.changed_lines(self.root, base, "HEAD", scope.test_only)
        self.assertIn("src/feature.rs", changed)
        self.assertNotIn("src/feature/tests/case.rs", changed)
        self.assertFalse(
            module.is_production_source("src/feature/tests/case.rs", scope.test_only)
        )
        runtime = implementation("scan_runtime")
        output = self.root / "coverage"
        with (
            patch.object(
                runtime,
                "rust_coverage",
                return_value=(
                    {
                        "src/feature.rs": {5: False},
                        "src/feature/tests/case.rs": {2: True},
                    },
                    scope,
                ),
            ),
            patch.object(runtime, "javascript_coverage", return_value={}),
        ):
            imported = runtime.coverage(
                self.root, self.root, output, {"test_timeout": 1}
            )
        self.assertEqual(imported.hits, {"src/feature.rs": {5: False}})

    def test_comment_cannot_exempt_compiled_rust_module_from_coverage(self):
        """A commented test attribute cannot hide a compiled production module."""
        module = implementation("git_snapshot")
        base = git(self.root, "rev-parse", "HEAD")
        owner = self.root / "src/feature.rs"
        owner.parent.mkdir()
        owner.write_text("/*\n#[cfg(test)]\nmod foo;\n*/\nmod foo;\n")
        production_module = self.root / "src/feature/foo.rs"
        production_module.parent.mkdir()
        production_module.write_text("pub fn live() -> u64 { 1 }\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-m", "add compiled Rust module")
        scope = module.RustSourceScope(
            frozenset({"src/feature.rs", "src/feature/foo.rs"}), frozenset()
        )
        changed = module.changed_lines(self.root, base, "HEAD", scope.test_only)
        self.assertIn("src/feature/foo.rs", changed)
        runtime = implementation("scan_runtime")
        with (
            patch.object(
                runtime,
                "rust_coverage",
                return_value=({"src/feature/foo.rs": {1: False}}, scope),
            ),
            patch.object(runtime, "javascript_coverage", return_value={}),
        ):
            imported = runtime.coverage(
                self.root, self.root, self.root / "coverage", {"test_timeout": 1}
            )
        self.assertEqual(imported.hits, {"src/feature/foo.rs": {1: False}})

    def test_compiled_rust_module_under_tests_path_remains_production(self):
        """A directory named tests cannot hide a production Rust module."""
        module = implementation("git_snapshot")
        base = git(self.root, "rev-parse", "HEAD")
        owner = self.root / "src/feature.rs"
        owner.parent.mkdir()
        owner.write_text(
            '#[path = "feature/tests/engine.rs"]\nmod engine;\n'
            "pub fn live() -> u64 { engine::value() }\n"
        )
        production_module = self.root / "src/feature/tests/engine.rs"
        production_module.parent.mkdir(parents=True)
        production_module.write_text("pub fn value() -> u64 { 1 }\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-m", "add compiled Rust module under tests")
        scope = module.RustSourceScope(
            frozenset({"src/feature.rs", "src/feature/tests/engine.rs"}),
            frozenset(),
        )
        changed = module.changed_lines(self.root, base, "HEAD", scope.test_only)
        self.assertIn("src/feature/tests/engine.rs", changed)
        runtime = implementation("scan_runtime")
        with (
            patch.object(
                runtime,
                "rust_coverage",
                return_value=({"src/feature/tests/engine.rs": {1: False}}, scope),
            ),
            patch.object(runtime, "javascript_coverage", return_value={}),
        ):
            imported = runtime.coverage(
                self.root, self.root, self.root / "coverage", {"test_timeout": 1}
            )
        self.assertEqual(imported.hits, {"src/feature/tests/engine.rs": {1: False}})

    def test_compiler_dependencies_prove_rust_test_only_scope(self):
        """A test-path file compiled by a non-test target never gets exempted."""
        module = implementation("git_snapshot")
        package = f"path+file://{self.root}#0.1.0"
        deps = self.root / "target/debug/deps"
        deps.mkdir(parents=True)
        (deps / "fixture-prod.d").write_text(
            "fixture-prod.d: src/lib.rs src/feature/tests/engine.rs\n"
        )
        (deps / "fixture-test.d").write_text(
            "fixture-test.d: src/lib.rs src/feature/tests/engine.rs "
            "src/feature/tests/case.rs\n"
        )
        artifacts = [
            {
                "reason": "compiler-artifact",
                "package_id": package,
                "filenames": [str(deps / f"libfixture-{kind}.rmeta")],
                "profile": {"test": kind == "test"},
            }
            for kind in ("prod", "test")
        ]
        serialized = "\n".join(json.dumps(artifact) for artifact in artifacts)
        scope = module.compiled_rust_scope(self.root, serialized)
        self.assertIn("src/feature/tests/engine.rs", scope.production)
        self.assertNotIn("src/feature/tests/engine.rs", scope.test_only)
        self.assertIn("src/feature/tests/case.rs", scope.test_only)
        (deps / "fixture-test.d").unlink()
        with self.assertRaisesRegex(RuntimeError, "SONAR_RUST_DEPFILE_MISSING"):
            module.compiled_rust_scope(self.root, serialized)
        (deps / "fixture-test.d").write_text("")
        with self.assertRaisesRegex(RuntimeError, "SONAR_RUST_DEPFILE_INVALID"):
            module.compiled_rust_scope(self.root, serialized)


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

    def test_missing_shell_report_is_not_a_zero_hit_report(self):
        module = implementation("coverage_report")
        changed = {".githooks/pre-push": {5, 6, 7}}
        with self.assertRaisesRegex(RuntimeError, "COVERAGE_MISSING"):
            module.changed_coverage(changed, {})
        mixed = {**changed, "src/lib.rs": {2}}
        self.assertEqual(
            module.changed_coverage(
                mixed,
                {"src/lib.rs": {2: True}, ".githooks/pre-push": {5: True, 7: False}},
            ),
            (2, 3),
        )
        snapshot = implementation("git_snapshot")
        self.assertTrue(snapshot.is_production_source("scripts/sonar-quality-gate.sh"))
        self.assertTrue(snapshot.is_production_source("tools/sonarqube/install.sh"))
        self.assertFalse(snapshot.is_production_source("README.md"))

    def test_uncovered_executable_shell_lines_still_block_the_gate(self):
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
