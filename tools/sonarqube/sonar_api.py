"""Bounded SonarQube reads and fail-closed incremental push admission."""

import collections
import hashlib
import json
import math
import re
import time
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """Never forward the project credential to a URL supplied by an HTTP redirect."""

    def redirect_request(self, request, response, code, message, headers, newurl):
        """Fail closed even for same-host redirects; the configured API is canonical."""
        raise RuntimeError("SONAR_API_REDIRECT_REJECTED")


def incremental_issues(baseline: list[dict], candidate: list[dict]) -> int:
    """Compare multiplicity of stable issue content, without storing source text."""

    def signatures(issues):
        return collections.Counter(
            hashlib.sha256(
                json.dumps(
                    [item.get(k) for k in ("rule", "component", "hash", "message")],
                    ensure_ascii=True,
                ).encode()
            ).hexdigest()
            for item in issues
        )

    return sum((signatures(candidate) - signatures(baseline)).values())


def require_pass(record: dict, tree: str, base: str, policy: str) -> None:
    """Admit only matching, complete evidence with zero findings and >=80% lines."""
    if any(
        record.get(k) != v
        for k, v in (("tree", tree), ("base", base), ("policy", policy))
    ):
        raise RuntimeError("SONAR_EVIDENCE_MISMATCH")
    if not record.get("analysis") or record.get("quality_gate") != "OK":
        raise RuntimeError("SONAR_QUALITY_GATE_FAILED")
    for key in ("new_issues", "covered", "coverable"):
        value = record.get(key)
        if type(value) is not int or value < 0:
            raise RuntimeError("SONAR_INCOMPLETE_EVIDENCE")
    if record["new_issues"]:
        raise RuntimeError("SONAR_INCREMENTAL_FINDINGS")
    if record["covered"] > record["coverable"] or record["covered"] < math.ceil(
        record["coverable"] * 0.8
    ):
        raise RuntimeError("SONAR_NEW_COVERAGE_BELOW_80")


class Sonar:
    """Own authenticated localhost-only requests and bounded task settlement."""

    def __init__(self, host: str, project: str, token: str):
        parsed = urllib.parse.urlsplit(host)
        if parsed.scheme != "http" or parsed.hostname not in {"localhost", "127.0.0.1"}:
            raise RuntimeError("SONAR_HOST_NOT_LOCAL")
        if not token:
            raise RuntimeError("SONAR_TOKEN_MISSING: export KODUCK_SONAR_TOKEN")
        if not re.fullmatch(r"[A-Za-z0-9._~-]+", token):
            raise RuntimeError("SONAR_TOKEN_INVALID")
        self.host, self.project, self.token = host.rstrip("/"), project, token
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), NoRedirect()
        )

    def get(self, endpoint: str, **params) -> dict:
        """Read JSON without echoing server error bodies or credentials."""
        url = self.host + endpoint + "?" + urllib.parse.urlencode(params)
        request = urllib.request.Request(
            url, headers={"Authorization": "Bearer " + self.token}
        )
        try:
            with self.opener.open(request, timeout=15) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            error.close()
            raise RuntimeError("SONAR_API_UNAVAILABLE: " + endpoint) from None
        except (OSError, ValueError):
            raise RuntimeError("SONAR_API_UNAVAILABLE: " + endpoint) from None

    def wait(self, task_id: str, seconds: int = 300) -> str:
        """Wait for this task's analysis ID; upload success alone is insufficient."""
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            task = self.get("/api/ce/task", id=task_id)["task"]
            if task["status"] == "SUCCESS" and task.get("analysisId"):
                return task["analysisId"]
            if task["status"] not in {"PENDING", "IN_PROGRESS"}:
                raise RuntimeError("SONAR_COMPUTE_FAILED")
            time.sleep(2)
        raise RuntimeError("SONAR_COMPUTE_TIMEOUT")

    def findings(self) -> list[dict]:
        """Read project-level unresolved issues, failing on incomplete pages."""
        endpoint = "/api/issues/search"
        params = {
            "components": self.project,
            "issueStatuses": "OPEN,CONFIRMED,ACCEPTED",
        }
        key = "issues"
        items = []
        for page in range(1, 21):
            data = self.get(endpoint, **params, p=page, ps=500)
            total = data["paging"]["total"]
            items.extend(data[key])
            if len(items) == total:
                return items
            if not data[key] or len(items) > total:
                break
        raise RuntimeError("SONAR_FINDINGS_INCOMPLETE")

    def gate(self, analysis: str) -> str:
        """Read the quality gate bound to an immutable analysis ID."""
        return self.get("/api/qualitygates/project_status", analysisId=analysis)[
            "projectStatus"
        ]["status"]

    def nonexecutable_files(self, names: set[str]) -> set[str]:
        """Confirm zero executable lines using project-level file metrics."""
        empty = set()
        for name in names:
            data = self.get(
                "/api/measures/component",
                component=self.project + ":" + name,
                metricKeys="lines_to_cover",
            )
            measures = data["component"]["measures"]
            values = [
                entry["value"]
                for entry in measures
                if entry["metric"] == "lines_to_cover"
            ]
            if values == ["0"]:
                empty.add(name)
        return empty
