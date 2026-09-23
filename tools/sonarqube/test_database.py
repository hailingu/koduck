"""Verify disposable database ownership and credential cleanup at the Docker boundary."""

import os
import unittest
from unittest.mock import patch

from test_gate import implementation


class DatabaseTests(unittest.TestCase):
    """Catch replacing supplied fixture URLs and leaking generated credentials."""

    def test_existing_isolated_url_is_preserved(self):
        module = implementation("postgres_fixture")
        with patch.dict(os.environ, {"KODUCK_AI_TEST_DATABASE_URL": "fixture-url"}):
            with module.database_fixture():
                self.assertEqual(
                    os.environ["KODUCK_AI_TEST_DATABASE_URL"], "fixture-url"
                )

    def test_owned_database_is_cleaned_on_failure_and_environment_restored(self):
        module = implementation("postgres_fixture")
        removed = []

        def external(command, *_args, **_kwargs):
            if command[1] == "port":
                return "127.0.0.1:45678\n"
            if command[1] == "rm":
                removed.append(command[-1])
            return ""

        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(module, "run", external),
        ):

            def fail_inside_fixture():
                with module.database_fixture():
                    self.assertIn(
                        "@127.0.0.1:45678/", os.environ["KODUCK_AI_TEST_DATABASE_URL"]
                    )
                    raise RuntimeError("fixture failure")

            with self.assertRaisesRegex(RuntimeError, "fixture failure"):
                fail_inside_fixture()
            self.assertNotIn("KODUCK_AI_TEST_DATABASE_URL", os.environ)
        self.assertEqual(len(removed), 1)
