"""Isolated Git snapshots; never stash, stage or reset the caller's work."""

import contextlib
import os
import re
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator


def git(root: Path, *args: str, index: bool = False) -> str:
    """Run Git with hook-local variables removed except an explicitly used index."""
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    if index and "GIT_INDEX_FILE" in os.environ:
        env["GIT_INDEX_FILE"] = os.environ["GIT_INDEX_FILE"]
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        env=env,
        capture_output=True,
        check=False,
        timeout=60,
    )
    if result.returncode:
        raise RuntimeError("SONAR_GIT_FAILED: " + args[0])
    return result.stdout.decode().strip()


@dataclass(frozen=True)
class Snapshot:
    """An immutable tree and its disposable checkout with usable SCM history."""

    path: Path
    revision: str
    tree: str


@contextlib.contextmanager
def revision_snapshot(root: Path, revision: str) -> Iterator[Snapshot]:
    """Check out an existing commit without changing the source repository."""
    with tempfile.TemporaryDirectory(prefix="koduck-sonar-") as directory:
        path = Path(directory) / "source"
        git(root, "clone", "--shared", "--no-checkout", str(root), str(path))
        git(path, "-c", "core.hooksPath=/dev/null", "checkout", "--detach", revision)
        yield Snapshot(path, revision, git(path, "rev-parse", "HEAD^{tree}"))


@contextlib.contextmanager
def index_snapshot(root: Path) -> Iterator[Snapshot]:
    """Materialize the effective index, including Git's partial-commit index."""
    tree = git(root, "write-tree", index=True)
    parent = git(root, "rev-parse", "HEAD")
    with revision_snapshot(root, parent) as checkout:
        revision = git(
            checkout.path,
            "-c",
            "user.name=Koduck Sonar Snapshot",
            "-c",
            "user.email=sonar@koduck.invalid",
            "commit-tree",
            tree,
            "-p",
            parent,
            "-m",
            "Disposable staged analysis snapshot",
        )
        git(checkout.path, "checkout", "--detach", revision)
        yield Snapshot(checkout.path, revision, tree)


def require_index(root: Path, expected: str) -> None:
    """Refuse to attach evidence to an index changed during analysis."""
    if git(root, "write-tree", index=True) != expected:
        raise RuntimeError("SONAR_INDEX_CHANGED: scan the new staged content")


def push_revisions(root: Path, text: str) -> list[str]:
    """Resolve every proposed ref target from Git pre-push stdin, including tags."""
    revisions = []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) != 4 or not re.fullmatch(r"[0-9a-f]{40,64}", fields[1]):
            raise RuntimeError("SONAR_PUSH_INPUT: invalid ref update")
        if set(fields[1]) == {"0"}:
            continue
        revision = git(root, "rev-parse", fields[1] + "^{commit}")
        if revision not in revisions:
            revisions.append(revision)
    return revisions


def feature_base(root: Path, revision: str) -> str:
    """Bind the feature to local dev's merge base, without implicit fetching."""
    return git(root, "merge-base", "dev", revision)


def changed_lines(root: Path, base: str, revision: str) -> dict[str, set[int]]:
    """Read added/modified line intervals without relying on quoted diff paths."""
    names = git(root, "diff", "--name-only", "--no-renames", "-z", base, revision)
    changes = {}
    for name in filter(None, names.split("\0")):
        if not is_production_source(name):
            continue
        diff = git(
            root,
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--unified=0",
            base,
            revision,
            "--",
            name,
        )
        lines = set()
        for start, count in re.findall(r"^@@ .* \+(\d+)(?:,(\d+))? @@", diff, re.M):
            lines.update(range(int(start), int(start) + int(count or "1")))
        if lines:
            changes[name] = lines
    return changes


def is_production_source(name: str) -> bool:
    """Select supported executable sources, excluding dedicated test fixtures."""
    path = Path(name)
    return (
        path.suffix in {".rs", ".py", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"}
        and not {"test", "tests", "fixtures"}.intersection(path.parts)
        and not path.name.startswith("test_")
        and ".test." not in path.name
    )
