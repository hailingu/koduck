#!/usr/bin/env python3
"""Owner-authorized commit scanning and zero-incremental-finding push gate."""

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import signal
import sys
import tempfile
from pathlib import Path

from coverage_report import changed_coverage, rust_declarations_only
from git_snapshot import (
    changed_lines,
    feature_base,
    git,
    index_snapshot,
    is_shell_source,
    push_revisions,
    require_index,
    revision_snapshot,
)
from scan_runtime import coverage, preflight, scan
from postgres_fixture import database_fixture
from sonar_api import Sonar, incremental_issues, require_pass

TOOLS = Path(__file__).resolve().parent
RUNNER_FILE_DIR = Path.home() / ".koduck"


def runner_file(name: str) -> str:
    """Read a secret delivered to the ephemeral worker over stdin.

    Hooks receive their credentials through the invoking shell; the
    ephemeral CI worker stores them in mode-0600 files written by the runner
    entrypoint so no job step inherits them through its environment.
    """
    try:
        return (RUNNER_FILE_DIR / name).read_text().strip()
    except OSError:
        return ""


def sonar_token() -> str:
    """Load the analysis token from the environment or the runner token file."""
    return os.environ.get("KODUCK_SONAR_TOKEN") or runner_file("sonar-token")


def restore_runner_database_url() -> None:
    """Restore the runner's fixture database URL when the environment omits it.

    The disposable worker receives the URL only through its mode-0600 file;
    the coverage fixture reads it from the environment of its own processes.
    """
    if not os.environ.get("KODUCK_AI_TEST_DATABASE_URL"):
        url = runner_file("database-url")
        if url:
            os.environ["KODUCK_AI_TEST_DATABASE_URL"] = url


def policy_id() -> str:
    """Bind cached evidence to the full executable scanner policy and pinned tools."""
    digest = hashlib.sha256()
    for path in sorted(TOOLS.iterdir()):
        if path.suffix in {".py", ".json", ".txt", ".sh"} and not path.name.startswith(
            "test_"
        ):
            digest.update(path.name.encode())
            digest.update(path.read_bytes())
    return digest.hexdigest()


def evidence_path(folder: Path, tree: str, base: str, policy: str) -> Path:
    """Use an unambiguous identity for one source/base/policy evidence record."""
    key = hashlib.sha256(json.dumps([tree, base, policy]).encode()).hexdigest()
    return folder / (key + ".json")


def store_evidence(folder: Path, record: dict) -> None:
    """Atomically persist non-secret results outside the versioned source tree."""
    folder.mkdir(parents=True, exist_ok=True)
    path = evidence_path(folder, record["tree"], record["base"], record["policy"])
    with tempfile.NamedTemporaryFile(mode="w", dir=folder, delete=False) as handle:
        json.dump(record, handle, sort_keys=True)
        temporary = handle.name
    os.replace(temporary, path)


def load_evidence(folder: Path, tree: str, base: str, policy: str) -> dict | None:
    """Read only an exactly keyed record; malformed local files never pass."""
    path = evidence_path(folder, tree, base, policy)
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        raise RuntimeError("SONAR_EVIDENCE_INVALID") from None


@contextlib.contextmanager
def project_lock():
    """Serialize all local clones sharing the single Community project."""
    path = Path(tempfile.gettempdir()) / "koduck-sonarqube.lock"
    descriptor = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise RuntimeError("SONAR_SCAN_BUSY") from None
        yield
    finally:
        os.close(descriptor)


def analyze(root: Path, snapshot, base: str, config: dict, sonar: Sonar) -> dict:
    """Compare base and candidate using the same analyzer, then bind all evidence."""
    policy = policy_id()
    preflight(config, TOOLS)
    with tempfile.TemporaryDirectory(prefix="koduck-sonar-results-") as temporary:
        output = Path(temporary)
        # Test before submitting either scan: verification failure retains the prior dashboard.
        hits = coverage(snapshot.path, TOOLS, output / "coverage", config)
        changed = changed_lines(snapshot.path, base, snapshot.revision)
        with revision_snapshot(root, base) as baseline:
            task, base_analysis = scan(baseline, config, sonar, output / "baseline")
            old_issues = sonar.findings()
        task, analysis = scan(
            snapshot,
            config,
            sonar,
            output / "candidate",
            output / "coverage/coverage.xml",
        )
        issues = sonar.findings()
        status = sonar.gate(analysis)
        missing = set(changed) - set(hits)
        declarations = {
            name
            for name in missing
            if name.endswith(".rs")
            and rust_declarations_only((snapshot.path / name).read_text())
        }
        # Sonar does not analyze shell scripts, so they never join the
        # file-metric classification; changed shell lines stay uncovered in
        # the denominator (see changed_coverage).
        shell = {name for name in missing if is_shell_source(name)}
        nonexecutable = declarations | sonar.nonexecutable_files(
            missing - declarations - shell
        )
        covered, coverable = changed_coverage(changed, hits, nonexecutable)
        if policy_id() != policy:
            raise RuntimeError("SONAR_POLICY_CHANGED: rescan with the final policy")
        return {
            "tree": snapshot.tree,
            "revision": snapshot.revision,
            "base": base,
            "base_analysis": base_analysis,
            "policy": policy,
            "task": task,
            "analysis": analysis,
            "quality_gate": status,
            "new_issues": incremental_issues(old_issues, issues),
            "covered": covered,
            "coverable": coverable,
        }


def report_result(record: dict) -> None:
    """Report safe evidence and distinguish successful analysis from push permission."""
    print(json.dumps(record, sort_keys=True), flush=True)
    try:
        require_pass(record, record["tree"], record["base"], record["policy"])
    except RuntimeError as error:
        print(
            str(error) + ": commit may retain findings; push remains blocked",
            flush=True,
        )


def check_revision(
    root: Path,
    revision: str,
    folder: Path,
    config: dict,
    sonar: Sonar,
    base: str | None = None,
) -> None:
    """Scan every actual proposed ref afresh; project-level evidence is never cached for admission."""
    base = base or feature_base(root, revision)
    tree, policy = git(root, "rev-parse", revision + "^{tree}"), policy_id()
    with revision_snapshot(root, revision) as snapshot:
        record = analyze(root, snapshot, base, config, sonar)
    store_evidence(folder, record)
    require_pass(record, tree, base, policy)
    print("Sonar push admitted: " + revision + " analysis=" + record["analysis"])


def main() -> int:
    """Dispatch hooks and CI through one policy, without performing a Git push."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["pre-commit", "pre-push", "check"])
    parser.add_argument("--revision", default="HEAD")
    parser.add_argument("--base")
    args = parser.parse_args()
    restore_runner_database_url()
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    folder = (
        Path(git(root, "rev-parse", "--path-format=absolute", "--git-common-dir"))
        / "sonarqube"
    )
    config = json.loads((TOOLS / "config.json").read_text())
    sonar = Sonar(config["host"], config["project"], sonar_token())
    with project_lock(), database_fixture():
        if args.mode == "pre-commit":
            with index_snapshot(root) as snapshot:
                base = feature_base(root, "HEAD")
                record = analyze(root, snapshot, base, config, sonar)
                require_index(root, snapshot.tree)
                store_evidence(folder, record)
                report_result(record)
        elif args.mode == "pre-push":
            for revision in push_revisions(root, sys.stdin.read()):
                check_revision(root, revision, folder, config, sonar)
        else:
            revision = git(root, "rev-parse", args.revision + "^{commit}")
            base = (
                git(root, "rev-parse", args.base + "^{commit}") if args.base else None
            )
            if base and git(root, "merge-base", base, revision) != base:
                raise RuntimeError("SONAR_BASE_NOT_ANCESTOR")
            check_revision(root, revision, folder, config, sonar, base)
    return 0


if __name__ == "__main__":

    def terminate(_signal, _frame):
        """Unwind owned processes, containers and snapshots on CI cancellation."""
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, terminate)
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("SONAR_GATE_CANCELLED", file=sys.stderr)
        sys.exit(130)
    except (RuntimeError, OSError, KeyError, ValueError) as failure:
        # RuntimeError messages are owned diagnostic codes. Never echo external exception text.
        print(
            str(failure)
            if isinstance(failure, RuntimeError)
            else "SONAR_GATE_UNAVAILABLE",
            file=sys.stderr,
        )
        sys.exit(1)
