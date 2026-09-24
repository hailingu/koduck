"""Check the Rust coverage command contract without compiling the service."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class RustCoverageScriptTests(unittest.TestCase):
    """Keep inline unit-test code out of the production LCOV run."""

    def test_only_named_integration_targets_produce_lcov(self):
        """A unit-test target must not be able to award production coverage."""
        script = Path(__file__).with_name("rust-coverage.sh")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "bin"
            binaries.mkdir()
            cargo = binaries / "cargo"
            cargo.write_text(
                "#!/bin/sh\n"
                'printf "%s\\n" "$@" > "$KODUCK_TEST_ARGS"\n'
                'while [ "$#" -gt 0 ]; do\n'
                '  if [ "$1" = "--output-path" ]; then\n'
                '    printf "SF:src/lib.rs\\nDA:1,1\\nend_of_record\\n" > "$2"\n'
                "    exit 0\n"
                "  fi\n"
                "  shift\n"
                "done\n"
                "exit 2\n"
            )
            cargo.chmod(0o755)
            args_file, report = root / "args", root / "rust.lcov"
            env = {
                **os.environ,
                "PATH": str(binaries) + os.pathsep + os.environ["PATH"],
                "KODUCK_TEST_ARGS": str(args_file),
            }
            result = subprocess.run(
                ["sh", str(script), str(report)],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("DA:1,1", report.read_text())
            args = args_file.read_text().splitlines()
            self.assertEqual(args[0], "llvm-cov")
            self.assertEqual(
                [
                    args[index + 1]
                    for index, arg in enumerate(args[:-1])
                    if arg == "--test"
                ],
                [
                    "cand_11_correction_admission",
                    "cand_12_projection",
                    "postgres_cand_11",
                ],
            )
            self.assertNotIn("--lib", args)
            self.assertNotIn("--all-targets", args)
            self.assertIn("--test-threads=3", args)


if __name__ == "__main__":
    unittest.main()
