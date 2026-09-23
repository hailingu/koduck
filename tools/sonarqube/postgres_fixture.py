"""Own a disposable PostgreSQL test container when no isolated URL is supplied."""

import contextlib
import os
import re
import secrets
import time
import uuid
from pathlib import Path

from scan_runtime import run


@contextlib.contextmanager
def database_fixture():
    """Generate ephemeral credentials and remove only this invocation's container."""
    if os.environ.get("KODUCK_AI_TEST_DATABASE_URL"):
        yield
        return
    name = "koduck-sonar-pg-" + uuid.uuid4().hex[:12]
    password = secrets.token_hex(24)
    root = Path.cwd()
    try:
        run(
            [
                "docker",
                "run",
                "--detach",
                "--name",
                name,
                "--env",
                "POSTGRES_USER=koduck",
                "--env",
                "POSTGRES_DB=koduck_test",
                "--env",
                "POSTGRES_PASSWORD",
                "-p",
                "127.0.0.1::5432",
                "postgres:18-alpine",
            ],
            root,
            120,
            {"POSTGRES_PASSWORD": password},
        )
        wait_ready(name, root)
        address = run(["docker", "port", name, "5432/tcp"], root, 15).strip()
        if not re.fullmatch(r"127\.0\.0\.1:\d+", address):
            raise RuntimeError("SONAR_DATABASE_ADDRESS_INVALID")
        os.environ["KODUCK_AI_TEST_DATABASE_URL"] = (
            f"postgresql://koduck:{password}@{address}/koduck_test"
        )
        yield
    finally:
        os.environ.pop("KODUCK_AI_TEST_DATABASE_URL", None)
        run(["docker", "rm", "--force", name], root, 30)


def wait_ready(name: str, root: Path) -> None:
    """Bound database startup without retrying tests or scanner submissions."""
    for _ in range(30):
        try:
            run(["docker", "exec", name, "pg_isready", "-U", "koduck"], root, 5)
            return
        except RuntimeError:
            time.sleep(1)
    raise RuntimeError("SONAR_DATABASE_TIMEOUT")
