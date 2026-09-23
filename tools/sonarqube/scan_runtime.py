"""Own disposable verification, coverage instrumentation and scanner processes."""

import os
import re
import signal
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

from coverage_report import read_lcov, write_generic
from git_snapshot import RustSourceScope, compiled_rust_scope, git, is_production_source


@dataclass(frozen=True)
class CoverageResult:
    """One snapshot's executable hits and compiler-proven Rust test scope."""

    hits: dict[str, dict[int, bool]]
    rust_scope: RustSourceScope


def run(
    command: list[str], cwd: Path, seconds: int = 1800, extra: dict | None = None
) -> str:
    """Bound a process group, suppress private output and contain scanner tokens."""
    env = {
        k: v
        for k, v in os.environ.items()
        if not k.startswith("GIT_")
        and k not in {"SONAR_TOKEN", "KODUCK_SONAR_TOKEN", "KODUCK_SHELL_COVERAGE"}
    }
    env.update(extra or {})
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    label = Path(command[0]).name
    if label == "cargo" and len(command) > 1:
        label += " " + command[1]
    print("Sonar check: " + label, flush=True)
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            result = process.wait(timeout=seconds)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise RuntimeError("SONAR_COMMAND_CANCELLED: " + label) from None
        if result:
            output.seek(0)
            text = output.read().decode(errors="replace")
            failed = re.findall(r"^test ([A-Za-z0-9_:]+) \.\.\. FAILED$", text, re.M)
            codes = re.findall(r"error\[(E\d+)\]", text)
            detail = ",".join(failed + codes)
            raise RuntimeError(
                "SONAR_COMMAND_FAILED: "
                + label
                + " (exit "
                + str(result)
                + ") "
                + detail
            )
        output.seek(0)
        return output.read().decode(errors="replace")


def task_id(report: Path, project: str) -> str:
    """Accept a scanner report only for the configured project with a task ID."""
    values = {
        key: value
        for line in report.read_text().splitlines()
        if "=" in line
        for key, value in [line.split("=", 1)]
    }
    if values.get("projectKey") != project or not values.get("ceTaskId"):
        raise RuntimeError("SONAR_REPORT_INVALID")
    return values["ceTaskId"]


def preflight(config: dict, tools: Path) -> None:
    """Require pinned tools before mutating the shared analysis project."""
    if config["scanner_version"] not in run(["sonar-scanner", "--version"], tools, 30):
        raise RuntimeError("SONAR_SCANNER_VERSION")
    if config["llvm_cov_version"] not in run(
        ["cargo", "llvm-cov", "--version"], tools, 30
    ):
        raise RuntimeError("SONAR_LLVM_COV_VERSION")
    if not os.environ.get("KODUCK_AI_TEST_DATABASE_URL"):
        raise RuntimeError("SONAR_DATABASE_MISSING: isolated PostgreSQL URL required")


def rust_coverage(
    snapshot: Path, output: Path, timeout: int
) -> tuple[dict, RustSourceScope]:
    """Verify Rust formatting/lints and exercise PostgreSQL tests with fresh LLVM coverage."""
    run(["cargo", "fmt", "--all", "--check"], snapshot, timeout)
    clippy_output = run(
        [
            "cargo",
            "clippy",
            "-p",
            "koduck-ai",
            "--all-targets",
            "--all-features",
            "--message-format=json",
            "--",
            "-D",
            "warnings",
        ],
        snapshot,
        timeout,
    )
    rust_scope = compiled_rust_scope(snapshot, clippy_output)
    rust = output / "rust.lcov"
    run(
        ["sh", str(snapshot / "tools/sonarqube/rust-coverage.sh"), str(rust)],
        snapshot,
        timeout,
    )
    return read_lcov(rust, snapshot), rust_scope


def coverage(snapshot: Path, output: Path, config: dict) -> CoverageResult:
    """Import product Rust coverage from one same-source integration run."""
    output.mkdir()
    timeout = config["test_timeout"]
    result, rust_scope = rust_coverage(snapshot, output, timeout)
    result = {
        path: hits
        for path, hits in result.items()
        if is_production_source(path, rust_scope.test_only)
    }
    write_generic(result, output / "coverage.xml")
    return CoverageResult(result, rust_scope)


def scan(
    snapshot, config: dict, sonar, output: Path, report: Path | None = None
) -> tuple[str, str]:
    """Submit one immutable snapshot and settle its exact compute-engine task."""
    output.mkdir()
    if git(snapshot.path, "diff", "HEAD", "--") or git(
        snapshot.path, "ls-files", "--others", "--exclude-standard"
    ):
        raise RuntimeError("SONAR_SNAPSHOT_CHANGED")
    properties = {
        "sonar.projectKey": config["project"],
        "sonar.host.url": sonar.host,
        "sonar.rust.cargo.manifestPaths": "koduck-ai/Cargo.toml",
        "sonar.sources": ".",
        "sonar.tests": ".",
        "sonar.exclusions": config["exclusions"] + "," + config["tests"],
        "sonar.coverage.exclusions": "tools/**,scripts/**,.githooks/**",
        "sonar.test.inclusions": config["tests"],
        "sonar.test.exclusions": config["exclusions"],
        "sonar.projectVersion": snapshot.tree,
        "sonar.scm.revision": snapshot.revision,
        "sonar.working.directory": str(output),
        "sonar.scanner.metadataFilePath": str(output / "report-task.txt"),
        "sonar.qualitygate.wait": "false",
        "sonar.scm.provider": "git",
        "sonar.scm.exclusions.disabled": "false",
    }
    if report:
        properties["sonar.coverageReportPaths"] = str(report)
    command = ["sonar-scanner"] + [
        "-D" + key + "=" + value for key, value in properties.items()
    ]
    run(
        command,
        snapshot.path,
        600,
        {"SONAR_TOKEN": sonar.token, "SONAR_HOST_URL": sonar.host},
    )
    task = task_id(output / "report-task.txt", config["project"])
    analysis = sonar.wait(task, config["compute_timeout"])
    return task, analysis
