"""Execute versioned hooks and the shared shell entry point without real scans."""

# ADR: docs/adr/ADR-0017-push-boundary-sonarqube-verification.md

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


def run_shell(script, arguments, cwd, environment, *, input=""):
    """Execute a copied hook fixture without invoking the live Sonar gate."""
    return subprocess.run(
        ["sh", str(script), *arguments],
        cwd=cwd,
        env=environment,
        input=input,
        text=True,
        capture_output=True,
        timeout=15,
        check=False,
    )


class HookTests(unittest.TestCase):
    """Catch hook bypasses, dropped ref stdin and wrong-project token selection."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.original = Path(__file__).resolve().parents[2]
        for name in [
            ".githooks/pre-push",
            "scripts/sonar-quality-gate.sh",
            "tools/sonarqube/install.sh",
        ]:
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.original / name, target)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        for option, value in (
            ("user.name", "Fixture"),
            ("user.email", "hook@koduck.invalid"),
        ):
            subprocess.run(
                ["git", "config", "--local", option, value], cwd=self.root, check=True
            )
        (self.root / "bin").mkdir()
        python = self.root / "bin/python3"
        python.write_text(
            '#!/bin/sh\nprintf "%s\\n" "$@" > "$FIXTURE_ROOT/args"\ncat > "$FIXTURE_ROOT/stdin"\n[ "$KODUCK_SONAR_TOKEN" = fixture-koduck ] || exit 24\nexit 23\n'
        )
        python.chmod(0o755)
        self.env = {
            **os.environ,
            "PATH": str(self.root / "bin") + ":" + os.environ["PATH"],
            "FIXTURE_ROOT": str(self.root),
            "KODUCK_SONAR_TOKEN": "fixture-koduck",
            "SONAR_TOKEN": "fixture-other-project",
            "ZDOTDIR": str(self.root),
        }

    def test_pre_push_propagates_gate_failure_and_push_stdin(self):
        result = run_shell(
            self.root / ".githooks" / "pre-push",
            [],
            self.root,
            self.env,
            input="ref-update\n",
        )
        self.assertEqual(result.returncode, 23)
        self.assertEqual((self.root / "args").read_text().splitlines()[-1], "pre-push")
        self.assertEqual((self.root / "stdin").read_text(), "ref-update\n")
        self.assertNotIn("fixture-koduck", result.stdout + result.stderr)

    def test_local_commits_activate_no_gate_resources(self):
        # AC-1: a managed checkout commits source and documentation freely.
        environment = {
            key: value
            for key, value in self.env.items()
            if key not in {"KODUCK_SONAR_TOKEN", "SONAR_TOKEN"}
        }
        for name in ("zsh", "cargo", "docker", "sonar-scanner"):
            sentinel = self.root / "bin" / name
            sentinel.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$0" >> "$FIXTURE_ROOT/sentinels"\nexit 99\n'
            )
            sentinel.chmod(0o755)
        script = self.root / "tools/sonarqube/install.sh"
        result = run_shell(script, [], self.root, self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        for message, name, content in (
            ("source", "code.py", "value = 1\n"),
            ("documentation", "doc.md", "# Fixture\n"),
        ):
            (self.root / name).write_text(content)
            subprocess.run(["git", "add", name], cwd=self.root, check=True)
            commit = subprocess.run(
                ["git", "commit", "-m", message],
                cwd=self.root,
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertEqual(commit.returncode, 0, commit.stderr)
            committed = subprocess.check_output(
                ["git", "show", "HEAD:" + name], cwd=self.root, text=True
            )
            self.assertEqual(committed, content)
        self.assertFalse((self.root / "args").exists())
        self.assertFalse((self.root / "sentinels").exists())

    @unittest.skipUnless(shutil.which("zsh"), "zsh fallback is macOS/local-only")
    def test_missing_export_loads_koduck_token_from_zshrc(self):
        self.env.pop("KODUCK_SONAR_TOKEN")
        (self.root / ".zshrc").write_text("export KODUCK_SONAR_TOKEN=fixture-koduck\n")
        result = run_shell(
            self.root / "scripts/sonar-quality-gate.sh",
            ["check", "--revision", "HEAD"],
            self.root,
            self.env,
        )
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(
            (self.root / "args").read_text().splitlines()[-3:],
            ["check", "--revision", "HEAD"],
        )

    def test_entry_point_defaults_to_manual_check(self):
        result = run_shell(
            self.root / "scripts/sonar-quality-gate.sh",
            [],
            self.root,
            self.env,
        )
        self.assertEqual(result.returncode, 23)
        self.assertEqual((self.root / "args").read_text().splitlines()[-1], "check")

    def test_installation_activates_push_hook_and_preserves_conflicting_hooks(self):
        # Git configuration and the versioned installer execute in a fixture.
        script = self.root / "tools/sonarqube/install.sh"
        result = run_shell(script, [], self.root, self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        configured = subprocess.check_output(
            ["git", "config", "--local", "core.hooksPath"],
            cwd=self.root,
            text=True,
        ).strip()
        self.assertEqual(configured, ".githooks")
        self.assertTrue(os.access(self.root / ".githooks/pre-push", os.X_OK))
        hook = run_shell(
            self.root / ".githooks/pre-push",
            [],
            self.root,
            self.env,
            input="ref-update\n",
        )
        self.assertEqual(hook.returncode, 23)
        self.assertEqual((self.root / "args").read_text().splitlines()[-1], "pre-push")
        result = run_shell(script, [], self.root, self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        subprocess.run(
            ["git", "config", "--local", "core.hooksPath", "existing-hooks"],
            cwd=self.root,
            check=True,
        )
        result = run_shell(script, [], self.root, self.env)
        self.assertEqual(result.returncode, 1)
        self.assertIn("SONAR_EXISTING_HOOKS", result.stderr)
        subprocess.run(
            ["git", "config", "--local", "--unset", "core.hooksPath"],
            cwd=self.root,
            check=True,
        )
        for hook_name in ("pre-commit", "pre-push"):
            (self.root / ".git/hooks" / hook_name).write_text("# existing hook\n")
            result = run_shell(script, [], self.root, self.env)
            self.assertEqual(result.returncode, 1)
            self.assertIn("SONAR_EXISTING_HOOKS", result.stderr)
            (self.root / ".git/hooks" / hook_name).unlink()


if __name__ == "__main__":
    unittest.main()
