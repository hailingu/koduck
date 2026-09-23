"""Trace Shell test commands and bind grammar-derived coverage to scanned sources."""

import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

from git_snapshot import git, is_production_source, is_shell_source

_GATE_ENTRYPOINT = "scripts/sonar-quality-gate.sh"
_HOOKS_CONFIG_KEY = "core.hooksPath"


def sources(root: Path) -> dict[str, bytes]:
    """Inventory maintained Shell files, including scripts no test executes."""
    return {
        name: (root / name).read_bytes()
        for name in git(root, "ls-files", "-z").split("\0")
        if name
        and is_shell_source(name)
        and is_production_source(name)
        and (root / name).is_file()
    }


def digest(source: bytes) -> str:
    """Bind execution evidence to the exact bytes of its maintained source."""
    return hashlib.sha256(source).hexdigest()


def parse(source: bytes):
    """Parse Shell grammar with the installed, pinned language bindings."""
    from tree_sitter import Language, Parser
    import tree_sitter_bash

    return Parser(Language(tree_sitter_bash.language())).parse(source).root_node


def signature(node) -> str:
    """Hash syntax tokens without retaining command text or insignificant whitespace."""
    pending, tokens = [node], []
    while pending:
        current = pending.pop()
        if current.type == "comment":
            continue
        if current.children:
            pending.extend(reversed(current.children))
        else:
            tokens.append((current.type, current.text.hex()))
    return digest(json.dumps(tokens).encode())


def command_sites(source: bytes) -> list[tuple]:
    """Locate commands and enclosing substitutions for DEBUG's end-line positions.

    Token signatures distinguish nested commands reported at the same closing
    line. Ambiguous matches stay uncovered. Comments, delimiters and argument
    or heredoc data are not executable commands.
    """
    root = parse(source)
    if root.has_error:
        raise RuntimeError("SONAR_SHELL_PARSE_FAILED")
    kinds = {
        "command",
        "variable_assignment",
        "variable_assignments",
        "declaration_command",
        "unset_command",
        "test_command",
        "for_statement",
        "c_style_for_statement",
        "case_statement",
    }
    pending, sites = [(root, None)], []
    while pending:
        node, enclosure = pending.pop()
        redirect_only = (
            node.type == "redirected_statement"
            and node.child_by_field_name("body") is None
        )
        arithmetic = node.type == "compound_statement" and node.children[0].type == "(("
        if node.type in kinds or redirect_only or arithmetic:
            start, end = node.start_point.row + 1, node.end_point.row + 1
            control = {"for_statement": "for", "case_statement": "case"}.get(
                node.type, ""
            )
            scope = enclosure or (start, end)
            sites.append((start, scope, signature(node), control))
            if not control:
                enclosure = scope
        pending.extend((child, enclosure) for child in node.named_children)
    return sites


def record_probe() -> None:
    """Consume command text over stdin and persist only a non-reversible signature."""
    working, filename, line, destination = sys.argv[2:]
    command = sys.stdin.buffer.read()
    node = parse(command)
    children = [child for child in node.named_children if child.type != "comment"]
    if len(children) == 1 and children[0].type == "redirected_statement":
        body = children[0].child_by_field_name("body")
        if body is not None:
            children = [body]
    fingerprint = (
        signature(children[0]) if not node.has_error and len(children) == 1 else ""
    )
    first = (
        command.lstrip().split(None, 1)[0].decode(errors="replace")
        if command.strip()
        else ""
    )
    control = first if first in {"for", "case"} else ""
    with Path(destination).open("a") as output:
        output.write(
            json.dumps([working, filename, int(line), fingerprint, control]) + "\n"
        )


def trace_environment(
    directory: Path, environment: dict, probe_python: Path | None = None
) -> dict:
    """Trace locations and signatures without logging commands or passing them in argv."""
    bash = shutil.which("bash")
    if not bash:
        raise RuntimeError("SONAR_SHELL_BASH_MISSING")
    version = subprocess.run(
        [bash, "-c", 'test "${BASH_VERSINFO[0]}" -ge 5'],
        env={"PATH": os.defpath},
        capture_output=True,
        check=False,
    )
    if version.returncode:
        raise RuntimeError("SONAR_SHELL_BASH_VERSION: Bash 5 or newer required")
    helper = directory / "trace-env"
    helper.write_text(
        "set -T\n"
        'trap \'printf "%s" "$BASH_COMMAND" | "$KODUCK_TRACE_PYTHON" '
        '"$KODUCK_TRACE_HELPER" probe "$PWD" "${BASH_SOURCE[0]}" '
        '"$LINENO" "$KODUCK_SHELL_TRACE"\' DEBUG\n'
    )
    binaries = directory / "bin"
    binaries.mkdir()
    for name in ("sh", "bash"):
        wrapper = binaries / name
        wrapper.write_text("#!/bin/sh\nexec " + shlex.quote(bash) + ' "$@"\n')
        wrapper.chmod(0o755)
    return {
        **environment,
        "PATH": str(binaries) + os.pathsep + environment.get("PATH", os.defpath),
        "BASH_ENV": str(helper),
        "KODUCK_SHELL_TRACE": str(directory / "trace"),
        "KODUCK_TRACE_PYTHON": str(probe_python or sys.executable),
        "KODUCK_TRACE_HELPER": str(Path(__file__).resolve()),
    }


def save_trace(
    directory: Path, report_directory: Path, cwd: Path, bindings: dict
) -> None:
    """Map fixture copies back to their source identities and reject source drift."""
    for name, fingerprint in bindings.items():
        if digest((cwd / name).read_bytes()) != fingerprint:
            raise RuntimeError("SONAR_SHELL_SOURCE_CHANGED")
    trace = directory / "trace"
    if not trace.is_file():
        raise RuntimeError("SONAR_SHELL_TRACE_MISSING")
    hits = set()
    for row in trace.read_text().splitlines():
        working, filename, line, fingerprint, control = json.loads(row)
        if not filename:
            continue
        path = Path(working) / filename
        try:
            name = path.resolve().relative_to(cwd.resolve()).as_posix()
        except ValueError:
            continue  # The generated tracer and fixture subprocesses are not source.
        if name in bindings:
            hits.add((name, line, fingerprint, control))
    report_directory.mkdir(parents=True, exist_ok=True)
    (report_directory / (uuid.uuid4().hex + ".json")).write_text(
        json.dumps({"sources": bindings, "hits": sorted(hits)})
    )


def run_shell(
    script: Path,
    arguments: list[str],
    cwd: Path,
    environment: dict,
    *,
    source_root: Path,
    report_directory: Path | None = None,
    probe_python: Path | None = None,
    input: str = "",
) -> subprocess.CompletedProcess:
    """Run a native Shell test normally, or trace its exact fixture copies for coverage."""
    location = report_directory or environment.get("KODUCK_SHELL_COVERAGE")
    command = ["sh", str(script.resolve()), *arguments]
    if not location:
        return subprocess.run(
            command,
            cwd=cwd,
            env=environment,
            input=input,
            text=True,
            capture_output=True,
            timeout=15,
            check=False,
        )
    bindings = {}
    for name, source in sources(source_root).items():
        copy = cwd / name
        if copy.is_file():
            if copy.read_bytes() != source:
                raise RuntimeError("SONAR_SHELL_SOURCE_CHANGED")
            bindings[name] = digest(source)
    with tempfile.TemporaryDirectory(prefix="koduck-shell-trace-") as temporary:
        directory = Path(temporary)
        result = subprocess.run(
            command,
            cwd=cwd,
            env=trace_environment(directory, environment, probe_python),
            input=input,
            text=True,
            capture_output=True,
            timeout=15,
            check=False,
        )
        save_trace(directory, Path(location), cwd, bindings)
    return result


def _verify_hook_entrypoints(execute, fixture: Path) -> None:
    """Check failure propagation, ref stdin, and token isolation for both hooks."""
    for hook in ("pre-commit", "pre-push"):
        result = execute(".githooks/" + hook, [], "ref-update\n")
        if (
            result.returncode != 23
            or (fixture / "args").read_text().splitlines()[-1:] != [hook]
            or (fixture / "stdin").read_text() != "ref-update\n"
            or "fixture-koduck" in result.stdout + result.stderr
        ):
            raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: hook")


def _verify_gate_entrypoint(execute, fixture: Path, environment: dict) -> None:
    """Check the manual default and missing-export fallback without a real scan."""
    result = execute(_GATE_ENTRYPOINT, [])
    if result.returncode != 23 or (fixture / "args").read_text().splitlines()[-1:] != [
        "check"
    ]:
        raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: entrypoint")
    if shutil.which("zsh"):
        environment.pop("KODUCK_SONAR_TOKEN")
        (fixture / ".zshrc").write_text("export KODUCK_SONAR_TOKEN=fixture-koduck\n")
        result = execute(_GATE_ENTRYPOINT, ["check", "--revision", "HEAD"])
        if result.returncode != 23 or (fixture / "args").read_text().splitlines()[
            -3:
        ] != ["check", "--revision", "HEAD"]:
            raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: fallback")
        environment["KODUCK_SONAR_TOKEN"] = "fixture-koduck"


def _verify_installation(execute, fixture: Path, binaries: Path) -> None:
    """Check idempotent local hook setup and refusal to replace another hook."""
    python = binaries / "python3"
    python.write_text(
        '#!/bin/sh\nmkdir -p "$3/bin"\n'
        'printf "#!/bin/sh\\nexit 0\\n" > "$3/bin/python"\n'
        'chmod +x "$3/bin/python"\n'
    )
    npm = binaries / "npm"
    npm.write_text("#!/bin/sh\nexit 0\n")
    npm.chmod(0o755)
    installer = "tools/sonarqube/install.sh"
    for _ in range(2):
        result = execute(installer, [])
        configured = subprocess.run(
            ["git", "config", "--local", "--get", _HOOKS_CONFIG_KEY],
            cwd=fixture,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode or configured.stdout.strip() != ".githooks":
            raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: install")
    subprocess.run(
        ["git", "config", "--local", _HOOKS_CONFIG_KEY, "existing-hooks"],
        cwd=fixture,
        check=True,
    )
    result = execute(installer, [])
    if result.returncode != 1 or "SONAR_EXISTING_HOOKS" not in result.stderr:
        raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: conflict")
    subprocess.run(
        ["git", "config", "--local", "--unset", _HOOKS_CONFIG_KEY],
        cwd=fixture,
        check=True,
    )
    (fixture / ".git/hooks/pre-commit").write_text("# existing hook\n")
    result = execute(installer, [])
    if result.returncode != 1 or "SONAR_EXISTING_HOOKS" not in result.stderr:
        raise RuntimeError("SONAR_SHELL_VERIFICATION_FAILED: existing hook")


def verify_shell_entrypoints(root: Path, reports: Path, probe_python: Path) -> None:
    """Produce gate traces from scanner-owned fixtures after candidate tests exit."""
    names = (
        ".githooks/pre-commit",
        ".githooks/pre-push",
        _GATE_ENTRYPOINT,
        "tools/sonarqube/install.sh",
    )
    inventory = sources(root)
    if any(name not in inventory for name in names):
        raise RuntimeError("SONAR_SHELL_ENTRYPOINT_MISSING")
    with tempfile.TemporaryDirectory(prefix="koduck-shell-verification-") as temporary:
        fixture = Path(temporary)
        for name in names:
            target = fixture / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(inventory[name])
        subprocess.run(["git", "init", "-q", str(fixture)], check=True)
        binaries = fixture / "bin"
        binaries.mkdir()
        python = binaries / "python3"
        python.write_text(
            '#!/bin/sh\nprintf "%s\\n" "$@" > "$FIXTURE_ROOT/args"\n'
            'cat > "$FIXTURE_ROOT/stdin"\n'
            '[ "$KODUCK_SONAR_TOKEN" = fixture-koduck ] || exit 24\nexit 23\n'
        )
        python.chmod(0o755)
        environment = {
            **{
                key: os.environ[key]
                for key in ("TMPDIR", "DEVELOPER_DIR")
                if key in os.environ
            },
            "PATH": str(binaries) + os.pathsep + os.environ.get("PATH", os.defpath),
            "HOME": str(fixture),
            "ZDOTDIR": str(fixture),
            "FIXTURE_ROOT": str(fixture),
            "KODUCK_SONAR_TOKEN": "fixture-koduck",
            "SONAR_TOKEN": "fixture-other-project",
        }

        def execute(
            name: str, arguments: list[str], input: str = ""
        ) -> subprocess.CompletedProcess:
            """Trace one exact fixture copy and keep its output private."""
            return run_shell(
                fixture / name,
                arguments,
                fixture,
                environment,
                source_root=root,
                report_directory=reports,
                probe_python=probe_python,
                input=input,
            )

        _verify_hook_entrypoints(execute, fixture)
        _verify_gate_entrypoint(execute, fixture, environment)
        _verify_installation(execute, fixture, binaries)


def matching_lines(
    commands: list[tuple], line: int, fingerprint: str, control: str
) -> set[int]:
    """Select grammar locations matching one probe without guessing ambiguous hits."""
    return {
        start
        for start, scope, expected, kind in commands
        if (control and control == kind and start == line)
        or (fingerprint and fingerprint == expected and scope[0] <= line <= scope[1])
    }


def read_trace(report: Path, inventory: dict[str, bytes]) -> dict:
    """Load one trace only after every reported source matches the current inventory."""
    record = json.loads(report.read_text())
    for name, fingerprint in record["sources"].items():
        if name not in inventory or digest(inventory[name]) != fingerprint:
            raise RuntimeError("SONAR_SHELL_SOURCE_CHANGED")
    return record


def collect(root: Path, reports: Path) -> dict[str, dict[int, bool]]:
    """Merge same-source hits over every executable script, keeping untouched lines false."""
    inventory = sources(root)
    sites = {name: command_sites(source) for name, source in inventory.items()}
    result = {
        name: dict.fromkeys((site[0] for site in commands), False)
        for name, commands in sites.items()
    }
    if not reports.is_dir():
        raise RuntimeError("SONAR_SHELL_REPORT_MISSING")
    for report in reports.glob("*.json"):
        record = read_trace(report, inventory)
        for name, line, fingerprint, control in record["hits"]:
            if name not in record["sources"]:
                raise RuntimeError("SONAR_SHELL_TRACE_INVALID")
            matches = matching_lines(sites[name], line, fingerprint, control)
            if len(matches) == 1:
                result[name][matches.pop()] = True
    return result


def main() -> None:
    """Emit LCOV from the instrumented test run using the installed grammar bindings."""
    root, reports, destination = map(Path, sys.argv[1:])
    coverage = collect(root, reports)
    lines = []
    for name, hits in sorted(coverage.items()):
        lines.append("SF:" + name)
        lines.extend(f"DA:{number},{int(hit)}" for number, hit in sorted(hits.items()))
        lines.append("end_of_record")
    destination.write_text("\n".join(lines))


if __name__ == "__main__":
    try:
        if sys.argv[1:2] == ["probe"]:
            record_probe()
        else:
            main()
    except (RuntimeError, ValueError, KeyError, OSError) as error:
        print(
            str(error)
            if isinstance(error, RuntimeError)
            else "SONAR_SHELL_REPORT_INVALID"
        )
        raise SystemExit(1) from None
