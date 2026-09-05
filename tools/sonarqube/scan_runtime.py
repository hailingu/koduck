"""Own disposable verification, coverage instrumentation and scanner processes."""

import os
import re
import signal
import subprocess
import tempfile
from pathlib import Path

from coverage_report import read_lcov, read_python, write_generic
from git_snapshot import git, is_production_source

VALIDATOR = "tools/governance-validator"


def _builder_credentials() -> tuple[int, int] | None:
    """Return the untrusted build uid/gid inside the ephemeral CI worker.

    The wrapper exports them only in the gate process, so instrumented
    commands execute PR-controlled build code as a uid that can neither read
    the gate-only analysis token nor the gate process environment. Local
    hooks and tests run without them and keep the invoking identity.
    """
    uid = os.environ.get("KODUCK_SONAR_BUILDER_UID")
    gid = os.environ.get("KODUCK_SONAR_BUILDER_GID")
    if uid and gid:
        return int(uid), int(gid)
    return None


def run(
    command: list[str], cwd: Path, seconds: int = 1800, extra: dict | None = None
) -> str:
    """Bound a process group, suppress private output and contain scanner tokens."""
    env = {
        k: v
        for k, v in os.environ.items()
        if not k.startswith("GIT_") and k not in {"SONAR_TOKEN", "KODUCK_SONAR_TOKEN"}
    }
    env.update(extra or {})
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    # Instrumented commands execute repository build code, so inside the
    # ephemeral worker they drop to the token-less builder uid; only the
    # scanner subprocess, which alone receives the analysis token, keeps the
    # gate identity.
    credentials = None if "SONAR_TOKEN" in env else _builder_credentials()

    def _drop() -> None:
        uid, gid = credentials
        os.setgid(gid)
        os.setuid(uid)

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
            preexec_fn=_drop if credentials else None,
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
    for path in (tools / ".venv/bin/python", tools / "node_modules/.bin/c8"):
        if not path.is_file():
            raise RuntimeError("SONAR_TOOLS_MISSING: run tools/sonarqube/install.sh")
    if not os.environ.get("KODUCK_AI_TEST_DATABASE_URL"):
        raise RuntimeError("SONAR_DATABASE_MISSING: isolated PostgreSQL URL required")


def rust_coverage(snapshot: Path, output: Path, timeout: int) -> dict:
    """Verify Rust formatting/lints and exercise PostgreSQL tests with fresh LLVM coverage."""
    run(["cargo", "fmt", "--all", "--check"], snapshot, timeout)
    run(
        [
            "cargo",
            "clippy",
            "-p",
            "koduck-ai",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        snapshot,
        timeout,
    )
    rust = output / "rust.lcov"
    run(
        [
            "cargo",
            "llvm-cov",
            "--locked",
            "-p",
            "koduck-ai",
            "--all-targets",
            "--all-features",
            "--lcov",
            "--output-path",
            str(rust),
            "--",
            "--test-threads=1",
        ],
        snapshot,
        timeout,
    )
    return read_lcov(rust, snapshot)


def javascript_coverage(
    snapshot: Path, tools: Path, output: Path, timeout: int
) -> dict:
    """Instrument validator subprocesses and preserve repository validation."""
    run(["npm", "ci", "--prefix", VALIDATOR], snapshot, timeout)
    js = output / "js"
    run(
        [
            str(tools / "node_modules/.bin/c8"),
            "--all",
            "--include",
            "tools/governance-validator/**/*.mjs",
            "--exclude",
            "**/test/**",
            "--reporter=lcov",
            "--reports-dir",
            str(js),
            "npm",
            "test",
            "--prefix",
            VALIDATOR,
        ],
        snapshot,
        timeout,
    )
    run(
        ["npm", "run", "validate", "--prefix", VALIDATOR],
        snapshot,
        timeout,
    )
    return read_lcov(js / "lcov.info", snapshot)


def python_coverage(snapshot: Path, tools: Path, output: Path, timeout: int) -> dict:
    """Instrument the hook workflow's Python tests and read their fresh report."""
    python = str(tools / ".venv/bin/python")
    extra = {"COVERAGE_FILE": str(output / ".coverage")}
    run(
        [
            python,
            "-m",
            "coverage",
            "run",
            "--source=tools/sonarqube",
            "--omit=*/test_*.py",
            "-m",
            "unittest",
            "discover",
            "-s",
            "tools/sonarqube",
            "-p",
            "test_*.py",
        ],
        snapshot,
        timeout,
        extra,
    )
    run(
        [python, "-m", "coverage", "xml", "-o", str(output / "python.xml")],
        snapshot,
        timeout,
        extra,
    )
    return read_python(output / "python.xml", snapshot)


def coverage(snapshot: Path, tools: Path, output: Path, config: dict) -> dict:
    """Run each supported language boundary and import one same-source report."""
    output.mkdir()
    # The instrumented commands run as the token-less builder uid inside the
    # ephemeral worker, so their report directory must accept their writes.
    os.chmod(output, 0o777)
    timeout = config["test_timeout"]
    result = rust_coverage(snapshot, output, timeout)
    result.update(javascript_coverage(snapshot, tools, output, timeout))
    if (snapshot / "tools/sonarqube/test_gate.py").exists():
        result.update(python_coverage(snapshot, tools, output, timeout))
    result = {path: hits for path, hits in result.items() if is_production_source(path)}
    write_generic(result, output / "coverage.xml")
    return result


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
