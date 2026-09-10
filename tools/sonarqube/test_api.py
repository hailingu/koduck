"""Exercise the real HTTP adapter against bounded Sonar protocol fixtures."""

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

from sonar_api import Sonar


class ApiTests(unittest.TestCase):
    """Catch stale tasks, malformed pages, authentication errors and false greens."""

    def setUp(self):
        responses = self.responses = {}

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                value = responses.get(urlsplit(self.path).path, (404, {}))
                status, body = value
                self.send_response(status)
                self.end_headers()
                self.wfile.write(json.dumps(body).encode())

            def log_message(self, *_args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.cleanup_server)
        self.sonar = Sonar(
            f"http://127.0.0.1:{self.server.server_port}", "koduck", "fixture-token"
        )

    def cleanup_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def test_exact_task_settlement_and_analysis_bound_gate(self):
        self.responses["/api/ce/task"] = (
            200,
            {"task": {"status": "SUCCESS", "analysisId": "analysis"}},
        )
        self.responses["/api/qualitygates/project_status"] = (
            200,
            {"projectStatus": {"status": "OK"}},
        )
        self.assertEqual(self.sonar.wait("task"), "analysis")
        self.assertEqual(self.sonar.gate("analysis"), "OK")
        self.responses["/api/ce/task"] = (200, {"task": {"status": "FAILED"}})
        with self.assertRaisesRegex(RuntimeError, "COMPUTE_FAILED"):
            self.sonar.wait("task")
        with self.assertRaisesRegex(RuntimeError, "COMPUTE_TIMEOUT"):
            self.sonar.wait("task", seconds=0)

    def test_complete_findings_and_empty_or_truncated_page(self):
        issue = {"rule": "rust:S1", "component": "koduck:a.rs", "message": "fixture"}
        self.responses["/api/issues/search"] = (
            200,
            {"paging": {"total": 1}, "issues": [issue]},
        )
        self.assertEqual(self.sonar.findings(), [issue])
        self.responses["/api/issues/search"] = (
            200,
            {"paging": {"total": 1}, "issues": []},
        )
        with self.assertRaisesRegex(RuntimeError, "FINDINGS_INCOMPLETE"):
            self.sonar.findings()

    def test_http_failure_is_not_empty_findings_or_a_token_diagnostic(self):
        self.responses["/api/issues/search"] = (403, {"error": "fixture-private-token"})
        with self.assertRaisesRegex(RuntimeError, "API_UNAVAILABLE") as raised:
            self.sonar.findings()
        self.assertNotIn("fixture-private-token", str(raised.exception))

    def test_nonlocal_hosts_and_missing_tokens_fail_before_io(self):
        for host, token in [
            ("https://example.com", "token"),
            ("http://localhost:9000", ""),
        ]:
            with self.assertRaises(RuntimeError):
                Sonar(host, "koduck", token)


if __name__ == "__main__":
    unittest.main()
