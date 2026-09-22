"""Exercise attainable, source-bound Shell coverage with real Bash processes."""

import os
import tempfile
import unittest
from pathlib import Path

from coverage_report import changed_coverage
from sonar_api import require_pass
from test_gate import git, implementation


class ShellCoverageTests(unittest.TestCase):
    """Keep executed commands distinct from data, comments and untouched branches."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "source"
        self.root.mkdir()
        git(self.root, "init", "-q")
        self.reports = Path(self.temporary.name) / "reports"
        self.reports.mkdir()

    def script(self, text, name="fixture.sh"):
        """Register a real parser fixture as maintained source in the isolated Git tree."""
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        git(self.root, "add", name)
        return path

    def trace(self, script, *arguments):
        """Drive the same instrumentation used by the hook integration suite."""
        return implementation("shell_coverage").run_shell(
            script,
            list(arguments),
            self.root,
            dict(os.environ),
            source_root=self.root,
            report_directory=self.reports,
        )

    def test_tested_shell_only_change_can_pass_and_unexecuted_branch_cannot(self):
        script = self.script(
            '#!/bin/sh\n# parser fixture\n\nif [ "$1" = yes ]; then\n'
            '  printf "yes\\n"\nelse\n  printf "no\\n"\nfi\n'
        )
        self.assertEqual(self.trace(script, "yes").stdout, "yes\n")
        module = implementation("shell_coverage")
        hits = module.collect(self.root, self.reports)
        # These line identities are fixture grammar locations, not prose/layout contracts.
        self.assertEqual(hits["fixture.sh"], {4: True, 5: True, 7: False})
        changed = {"fixture.sh": set(range(1, 9))}
        covered, coverable = changed_coverage(changed, hits)
        record = dict(
            tree="t",
            base="b",
            policy="p",
            analysis="a",
            quality_gate="OK",
            new_issues=0,
            covered=covered,
            coverable=coverable,
        )
        with self.assertRaisesRegex(RuntimeError, "COVERAGE_BELOW_80"):
            require_pass(record, "t", "b", "p")
        self.assertEqual(self.trace(script, "no").stdout, "no\n")
        record["covered"], record["coverable"] = changed_coverage(
            changed, module.collect(self.root, self.reports)
        )
        require_pass(record, "t", "b", "p")

    def test_unexecuted_and_comment_only_scripts_have_distinct_reports(self):
        self.script('#!/bin/sh\nprintf "not executed\\n"\n')
        self.script("#!/bin/sh\n# deliberately empty\n\n", "empty.sh")
        hits = implementation("shell_coverage").collect(self.root, self.reports)
        self.assertEqual(hits, {"empty.sh": {}, "fixture.sh": {2: False}})

    def test_changed_source_invalidates_recorded_hits(self):
        script = self.script('printf "before\\n"\n')
        self.trace(script)
        script.write_text('printf "after\\n"\n')
        module = implementation("shell_coverage")
        with self.assertRaisesRegex(RuntimeError, "SHELL_SOURCE_CHANGED"):
            module.collect(self.root, self.reports)

    def test_redirection_and_arithmetic_are_not_zero_executable_lines(self):
        script = self.script(
            '> output\ncount=1\n(( count += 1 ))\nprintf "%s\\n" "$count"\n'
        )
        self.assertEqual(self.trace(script).stdout, "2\n")
        self.assertTrue((self.root / "output").is_file())
        hits = implementation("shell_coverage").collect(self.root, self.reports)
        self.assertEqual(hits["fixture.sh"], {1: True, 2: True, 3: True, 4: True})

    def test_functions_substitutions_multiline_arguments_and_heredoc_data(self):
        script = self.script(
            'unused() {\n  printf "unreached\\n"\n}\n'
            'value=$(\n  printf "secret-fixture"\n)\n'
            'printf "%s\\n" \\\n  "$value"\n'
            "cat <<'EOF'\nthis is data, not a command\nEOF\n"
        )
        result = self.trace(script)
        self.assertEqual(result.stdout, "secret-fixture\nthis is data, not a command\n")
        hits = implementation("shell_coverage").collect(self.root, self.reports)
        self.assertEqual(
            hits["fixture.sh"], {2: False, 4: True, 5: True, 7: True, 9: True}
        )
        for report in self.reports.glob("*.json"):
            self.assertNotIn("secret-fixture", report.read_text())

    def test_nested_scripts_and_exit_status_are_preserved(self):
        self.script('printf "child\\n"\nexit 7\n', "child.sh")
        script = self.script('sh "$PWD/child.sh"\nstatus=$?\nexit "$status"\n')
        result = self.trace(script)
        self.assertEqual((result.returncode, result.stdout), (7, "child\n"))
        hits = implementation("shell_coverage").collect(self.root, self.reports)
        self.assertTrue(all(hit for file in hits.values() for hit in file.values()))

    def test_invalid_shell_syntax_and_absent_reports_fail_closed(self):
        module = implementation("shell_coverage")
        self.script("if then\n")
        with self.assertRaisesRegex(RuntimeError, "SHELL_PARSE_FAILED"):
            module.collect(self.root, self.reports)
        self.script("# no executable commands\n")
        missing = self.reports / "missing"
        with self.assertRaisesRegex(RuntimeError, "SHELL_REPORT_MISSING"):
            module.collect(self.root, missing)
