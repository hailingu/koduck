#!/usr/bin/env python3
"""Build and run one isolated Docker-based GitHub Actions worker for Koduck."""

import argparse
import json
import os
import re
import secrets
import subprocess
import time
import uuid
from pathlib import Path

REPOSITORY = "hailingu/koduck"
IMAGE = "koduck-sonarqube-runner:2.337.0"


def command(
    args: list[str],
    data: str | None = None,
    timeout: int = 120,
    extra: dict | None = None,
) -> str:
    """Capture bootstrap output so credentials and Docker metadata are not logged."""
    env = {
        k: v
        for k, v in os.environ.items()
        if k not in {"SONAR_TOKEN", "KODUCK_SONAR_TOKEN"}
    }
    env.update(extra or {})
    result = subprocess.run(
        args,
        input=data,
        capture_output=True,
        text=True,
        env=env,
        timeout=timeout,
        check=False,
    )
    if result.returncode:
        raise RuntimeError("SONAR_RUNNER_COMMAND_FAILED: " + Path(args[0]).name)
    return result.stdout.strip()


def container_command(name: str, network: str) -> list[str]:
    """Pass secret variable names, never their values, to the disposable worker."""
    if not all(re.fullmatch(r"[a-zA-Z0-9_-]+", value) for value in (name, network)):
        raise RuntimeError("SONAR_RUNNER_NAME_INVALID")
    return [
        "docker",
        "run",
        "--rm",
        "-i",
        "--user",
        "root",
        "--name",
        name,
        "--network",
        network,
        "--add-host",
        "host.docker.internal:host-gateway",
        IMAGE,
    ]


def ready_database(name: str, network: str, password: str) -> None:
    """Start a no-volume PostgreSQL fixture and wait for its bounded readiness."""
    command(
        [
            "docker",
            "run",
            "--detach",
            "--rm",
            "--name",
            name,
            "--network",
            network,
            "--env",
            "POSTGRES_USER=koduck",
            "--env",
            "POSTGRES_DB=koduck_test",
            "--env",
            "POSTGRES_PASSWORD",
            "postgres:18-alpine",
        ],
        extra={"POSTGRES_PASSWORD": password},
    )
    for _ in range(30):
        try:
            command(["docker", "exec", name, "pg_isready", "-U", "koduck"], timeout=5)
            return
        except RuntimeError:
            time.sleep(1)
    raise RuntimeError("SONAR_RUNNER_DATABASE_TIMEOUT")


def launch(name: str, token: str) -> None:
    """Register one JIT worker; remove containers/network even after interruption."""
    network, database = name + "-network", name + "-postgres"
    password = secrets.token_hex(24)
    command(["docker", "network", "create", network])
    try:
        ready_database(database, network, password)
        body = {
            "name": name,
            "runner_group_id": 1,
            "labels": ["self-hosted", "koduck-sonarqube"],
            "work_folder": "_work",
        }
        response = json.loads(
            command(
                [
                    "gh",
                    "api",
                    "--method",
                    "POST",
                    "repos/" + REPOSITORY + "/actions/runners/generate-jitconfig",
                    "--input",
                    "-",
                ],
                json.dumps(body),
            )
        )
        command(
            [
                "gh",
                "variable",
                "set",
                "KODUCK_SONAR_RUNNER_ENABLED",
                "--repo",
                REPOSITORY,
                "--body",
                "true",
            ]
        )
        print("Ephemeral runner registered: " + name, flush=True)
        # The analysis token reaches the worker over stdin, never as a
        # container environment variable, so no job step inherits it; the
        # entrypoint stores it in a mode-0600 file for the gate step alone.
        command(
            container_command(name, network),
            response["encoded_jit_config"]
            + "\n"
            + token
            + "\n"
            + f"postgresql://koduck:{password}@{database}:5432/koduck_test"
            + "\n",
            timeout=7200,
        )
    finally:
        failures = []
        for args in (
            ["docker", "rm", "--force", name],
            ["docker", "rm", "--force", database],
            ["docker", "network", "rm", network],
            [
                "gh",
                "variable",
                "set",
                "KODUCK_SONAR_RUNNER_ENABLED",
                "--repo",
                REPOSITORY,
                "--body",
                "false",
            ],
        ):
            try:
                command(args)
            except RuntimeError:
                failures.append(Path(args[0]).name)
        if failures:
            print(
                "SONAR_RUNNER_CLEANUP_CHECK_REQUIRED: " + ",".join(failures), flush=True
            )


def main() -> None:
    """Build pinned tooling, optionally stopping before GitHub registration."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-only", action="store_true")
    args = parser.parse_args()
    tools = Path(__file__).resolve().parent
    print("Building the disposable SonarQube runner image", flush=True)
    command(
        [
            "docker",
            "build",
            "--file",
            str(tools / "runner.Dockerfile"),
            "--tag",
            IMAGE,
            str(tools),
        ],
        timeout=3600,
    )
    if not args.build_only:
        token = os.environ.get("KODUCK_SONAR_TOKEN")
        if not token:
            raise RuntimeError("SONAR_TOKEN_MISSING")
        launch("koduck-sonar-" + uuid.uuid4().hex[:12], token)


if __name__ == "__main__":
    try:
        main()
    except (
        RuntimeError,
        OSError,
        ValueError,
        KeyError,
        subprocess.TimeoutExpired,
    ) as error:
        print(
            str(error)
            if isinstance(error, RuntimeError)
            else "SONAR_RUNNER_UNAVAILABLE"
        )
        raise SystemExit(1) from None
