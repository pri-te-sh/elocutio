#!/usr/bin/env python3
"""Tests for the progress tracker and dashboard generator.

Run with:

    python3 -m unittest discover -s scripts -p "test_*.py"

The tests are standard-library only and do not touch the real tracker; every
case builds a small synthetic repository in a temporary directory.
"""

from __future__ import annotations

import copy
import json
import shutil
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import progress  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = REPO_ROOT / "progress" / "template.html"


def synthetic_tracker() -> dict:
    """Return a minimal but valid tracker for the tests."""
    return {
        "schemaVersion": 1,
        "updatedAt": "2026-10-19T12:00:00Z",
        "project": {
            "name": "Testcase",
            "tagline": "Test the tracker.",
            "status": "pre-alpha",
            "startDate": "2026-10-19",
            "repo": "example/testcase",
            "plan": "docs/PLAN.md",
            "roadmap": "ROADMAP.md",
            "planWeeks": [14, 19],
        },
        "areas": {"planning": "Planning", "build": "Build", "qa": "Quality"},
        "priorities": {"P0": "Gate-critical", "P1": "Core"},
        "milestones": [
            {
                "id": "M0",
                "name": "First phase",
                "short": "First",
                "outcome": "Prove the tracker works.",
                "estimateDays": [2, 4],
                "weeks": [1, 1],
                "tasks": [
                    {
                        "id": "m0-alpha",
                        "title": "Write alpha",
                        "area": "build",
                        "priority": "P0",
                        "estimateDays": 1,
                        "evidence": [{"type": "path", "path": "alpha.txt"}],
                    },
                    {
                        "id": "m0-beta",
                        "title": "Write both files",
                        "area": "build",
                        "priority": "P0",
                        "estimateDays": 1,
                        "evidence": [
                            {"type": "path", "path": "alpha.txt"},
                            {"type": "path", "path": "beta.txt"},
                        ],
                    },
                    {
                        "id": "m0-gamma",
                        "title": "Unstarted work",
                        "area": "planning",
                        "priority": "P1",
                        "estimateDays": 1,
                        "dependsOn": ["m0-beta"],
                    },
                ],
                "acceptance": [
                    {"text": "Alpha exists", "accepted": True},
                    {"text": "Beta exists", "accepted": False},
                ],
            },
            {
                "id": "M1",
                "name": "Second phase",
                "short": "Second",
                "outcome": "Ship it.",
                "estimateDays": [4, 6],
                "weeks": [2, 2],
                "tasks": [
                    {
                        "id": "m1-delta",
                        "title": "Add a matching commit",
                        "area": "qa",
                        "priority": "P1",
                        "estimateDays": 2,
                        "evidence": [{"type": "git-commit", "pattern": "^feat:"}],
                    }
                ],
                "acceptance": [{"text": "Commit exists", "accepted": False}],
            },
        ],
        "blockers": [],
        "risks": [
            {
                "id": "R1",
                "risk": "Test risk",
                "mitigation": "Test mitigation",
                "severity": "medium",
                "area": "build",
            }
        ],
        "history": [],
    }


class TrackerTestCase(unittest.TestCase):
    """Base class that builds a temporary repository root."""

    def setUp(self) -> None:
        self.temp = Path(tempfile.mkdtemp(prefix="elocutio-progress-"))
        (self.temp / "progress").mkdir(parents=True)
        (self.temp / "docs").mkdir()
        (self.temp / "docs" / "PLAN.md").write_text("# Plan\n", encoding="utf-8")
        (self.temp / "ROADMAP.md").write_text("# Roadmap\n", encoding="utf-8")
        shutil.copyfile(TEMPLATE, self.temp / "progress" / "template.html")
        self.tracker = synthetic_tracker()

    def tearDown(self) -> None:
        shutil.rmtree(self.temp, ignore_errors=True)

    def write_tracker(self, tracker: dict | None = None) -> Path:
        path = self.temp / "progress" / "tracker.json"
        path.write_text(
            json.dumps(tracker if tracker is not None else self.tracker, indent=2) + "\n",
            encoding="utf-8",
        )
        return path

    def build_payload(self, tracker: dict | None = None) -> dict:
        self.write_tracker(tracker)
        return progress.build_payload(self.temp, progress.load_tracker(self.temp))


class ValidationTests(TrackerTestCase):
    def test_valid_tracker_passes(self) -> None:
        self.assertEqual(progress.validate_tracker(synthetic_tracker()), [])

    def test_duplicate_task_id_is_rejected(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["milestones"][1]["tasks"][0]["id"] = "m0-alpha"
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("duplicate task id" in error for error in errors))

    def test_unknown_area_and_dependency_are_rejected(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["milestones"][0]["tasks"][0]["area"] = "not-an-area"
        tracker["milestones"][0]["tasks"][2]["dependsOn"] = ["does-not-exist"]
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("not declared in areas" in error for error in errors))
        self.assertTrue(any("does not exist" in error for error in errors))

    def test_bad_status_and_acceptance_type_are_rejected(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["milestones"][0]["tasks"][0]["status"] = "finished"
        tracker["milestones"][0]["acceptance"][0]["accepted"] = "yes"
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("unknown status" in error for error in errors))
        self.assertTrue(any("boolean accepted" in error for error in errors))

    def test_unsafe_evidence_selectors_are_rejected(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["milestones"][0]["tasks"][0]["evidence"] = [
            {"type": "glob", "pattern": "/etc/*"},
            {"type": "path", "path": "../outside.txt"},
        ]
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("path must stay relative" in error for error in errors))
        self.assertTrue(any("pattern must stay relative" in error for error in errors))

    def test_plan_weeks_must_be_integers(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["project"]["planWeeks"] = ["a", "b"]
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("planWeeks" in error for error in errors))

    def test_repo_slug_is_validated(self) -> None:
        tracker = copy.deepcopy(synthetic_tracker())
        tracker["project"]["repo"] = "javascript:alert(1)"
        errors = progress.validate_tracker(tracker)
        self.assertTrue(any("project.repo" in error for error in errors))


class EvidenceTests(TrackerTestCase):
    def test_evidence_derives_statuses(self) -> None:
        (self.temp / "alpha.txt").write_text("alpha", encoding="utf-8")
        payload = self.build_payload()
        tasks = {task["id"]: task for milestone in payload["milestones"] for task in milestone["tasks"]}

        self.assertEqual(tasks["m0-alpha"]["status"], "done")
        self.assertEqual(tasks["m0-alpha"]["source"], "evidence")
        self.assertEqual(tasks["m0-beta"]["status"], "in_progress")
        self.assertEqual(tasks["m0-gamma"]["status"], "planned")
        self.assertEqual(tasks["m0-gamma"]["waitingOn"][0]["id"], "m0-beta")

    def test_missing_evidence_is_planned(self) -> None:
        payload = self.build_payload()
        tasks = {task["id"]: task for milestone in payload["milestones"] for task in milestone["tasks"]}
        self.assertEqual(tasks["m0-alpha"]["status"], "planned")
        self.assertEqual(tasks["m0-alpha"]["source"], "evidence")
        self.assertEqual(tasks["m0-alpha"]["evidencePassed"], 0)

    def test_manual_override_wins_over_evidence(self) -> None:
        (self.temp / "alpha.txt").write_text("alpha", encoding="utf-8")
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"][0]["status"] = "blocked"
        payload = self.build_payload(tracker)
        tasks = {task["id"]: task for milestone in payload["milestones"] for task in milestone["tasks"]}
        self.assertEqual(tasks["m0-alpha"]["status"], "blocked")
        self.assertEqual(tasks["m0-alpha"]["source"], "manual")
        self.assertTrue(tasks["m0-alpha"]["drift"])

    def test_drift_when_manual_status_lags_evidence(self) -> None:
        (self.temp / "alpha.txt").write_text("alpha", encoding="utf-8")
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"][0]["status"] = "planned"
        payload = self.build_payload(tracker)
        tasks = {task["id"]: task for milestone in payload["milestones"] for task in milestone["tasks"]}
        self.assertEqual(tasks["m0-alpha"]["status"], "planned")
        self.assertEqual(tasks["m0-alpha"]["suggestedStatus"], "done")
        self.assertTrue(tasks["m0-alpha"]["drift"])

    def test_text_and_glob_evidence(self) -> None:
        (self.temp / "docs" / "PLAN.md").write_text("# Plan\n\n## Milestone\n", encoding="utf-8")
        (self.temp / "src").mkdir()
        (self.temp / "src" / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"] = [
            {
                "id": "m0-alpha",
                "title": "Marker",
                "area": "build",
                "priority": "P0",
                "estimateDays": 1,
                "evidence": [
                    {"type": "text", "path": "docs/PLAN.md", "contains": "## Milestone"},
                    {"type": "glob", "pattern": "src/*.rs"},
                ],
            }
        ]
        payload = self.build_payload(tracker)
        task = payload["milestones"][0]["tasks"][0]
        self.assertEqual(task["status"], "done")
        self.assertTrue(all(item["ok"] for item in task["evidence"]))


class ProgressTests(TrackerTestCase):
    def test_progress_is_weighted_by_estimate(self) -> None:
        (self.temp / "alpha.txt").write_text("alpha", encoding="utf-8")
        payload = self.build_payload()
        milestone = payload["milestones"][0]
        # Only the first task is done: 1 of 3 estimate days.
        self.assertAlmostEqual(milestone["progress"], 1 / 3, places=3)
        self.assertGreater(payload["summary"]["overallProgress"], 0)
        self.assertLess(payload["summary"]["overallProgress"], 1)

    def test_plan_window_and_expected_progress(self) -> None:
        payload = self.build_payload()
        summary = payload["summary"]
        self.assertEqual(summary["planWindow"]["start"], "2026-10-19")
        self.assertEqual(summary["planWindow"]["end"], "2027-02-28")
        self.assertGreaterEqual(summary["expectedProgress"], 0)
        self.assertLessEqual(summary["expectedProgress"], 1)
        self.assertEqual(summary["tasks"]["total"], 4)
        self.assertEqual(summary["milestonesTotal"], 2)

    def test_revision_changes_with_status(self) -> None:
        first = self.build_payload()
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"][0]["status"] = "done"
        second = self.build_payload(tracker)
        self.assertNotEqual(first["rev"], second["rev"])

    def test_revision_changes_with_any_ledger_edit(self) -> None:
        first = self.build_payload()
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"][0]["notes"] = "A note that changes no status."
        second = self.build_payload(tracker)
        self.assertNotEqual(first["rev"], second["rev"])

    def test_phase_status_uses_derived_status(self) -> None:
        self.write_tracker()
        payload = progress.build_payload(self.temp, progress.load_tracker(self.temp))
        first_open = next(milestone for milestone in payload["milestones"] if milestone["status"] != "done")
        self.assertEqual(payload["summary"]["phaseStatus"], first_open["status"])


class MutationTests(TrackerTestCase):
    def test_update_and_clear_manual_status(self) -> None:
        self.write_tracker()
        old, new = progress.update_task_status(self.temp, "m0-alpha", "done", "shipped")
        self.assertIsNone(old)
        self.assertEqual(new, "done")

        tracker = progress.load_tracker(self.temp)
        task = tracker["milestones"][0]["tasks"][0]
        self.assertEqual(task["status"], "done")
        self.assertEqual(tracker["history"][-1]["note"], "shipped")
        self.assertIsNotNone(tracker["updatedAt"])

        progress.update_task_status(self.temp, "m0-alpha", "auto")
        tracker = progress.load_tracker(self.temp)
        self.assertNotIn("status", tracker["milestones"][0]["tasks"][0])
        self.assertEqual(tracker["history"][-1]["to"], "auto")

    def test_unknown_task_and_status_are_rejected(self) -> None:
        self.write_tracker()
        with self.assertRaises(progress.ProgressError):
            progress.update_task_status(self.temp, "nope", "done")
        with self.assertRaises(progress.ProgressError):
            progress.update_task_status(self.temp, "m0-alpha", "finished")

    def test_invalid_tracker_fails_build(self) -> None:
        tracker = copy.deepcopy(self.tracker)
        tracker["milestones"][0]["tasks"][0]["area"] = "nope"
        self.write_tracker(tracker)
        with self.assertRaises(progress.ProgressError):
            progress.build_payload(self.temp, progress.load_tracker(self.temp))


class RenderTests(TrackerTestCase):
    def test_html_contains_escaped_bootstrap(self) -> None:
        payload = self.build_payload()
        payload["project"]["description"] = '</script><script>alert("x")</script>'
        html = progress.render_html(TEMPLATE.read_text(encoding="utf-8"), payload)
        self.assertIn("const BOOTSTRAP = {", html)
        self.assertNotIn("</script><script>alert", html)
        self.assertIn("\\u003c/script>", html)

    def test_html_escapes_comment_openers(self) -> None:
        payload = self.build_payload()
        payload["project"]["description"] = "<!--<script>alert('x')</script>"
        html = progress.render_html(TEMPLATE.read_text(encoding="utf-8"), payload)
        self.assertNotIn("<!--<script>", html)
        self.assertIn("\\u003c!--\\u003cscript>", html)

    def test_build_writes_static_dashboard(self) -> None:
        self.write_tracker()
        html_path, json_path = progress.build(self.temp, self.temp / "progress" / "out")
        self.assertTrue(html_path.is_file())
        self.assertTrue(json_path.is_file())
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        self.assertEqual(payload["mode"], "static")
        self.assertEqual(payload["refreshSeconds"], 0)
        html = html_path.read_text(encoding="utf-8")
        self.assertNotIn("/*__BOOTSTRAP__*/ null", html)
        self.assertIn('"mode":"static"', html)


class ServerTests(TrackerTestCase):
    def start_server(self) -> tuple[ThreadingHTTPServer, threading.Thread, str]:
        self.write_tracker()
        template = (self.temp / "progress" / "template.html").read_text(encoding="utf-8")
        handler = progress.make_handler(self.temp, template)
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        return server, thread, f"http://127.0.0.1:{server.server_address[1]}"

    def stop_server(self, server: ThreadingHTTPServer, thread: threading.Thread) -> None:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)

    def test_api_and_patch_round_trip(self) -> None:
        server, thread, base = self.start_server()
        try:
            with urllib.request.urlopen(base + "/api/progress") as response:
                payload = json.load(response)
            self.assertEqual(payload["mode"], "live")
            self.assertEqual(response.headers["Cache-Control"], "no-store, max-age=0")

            request = urllib.request.Request(
                base + "/api/tasks/m0-alpha",
                data=json.dumps({"status": "done"}).encode("utf-8"),
                headers={"Content-Type": "application/json"},
                method="PATCH",
            )
            with urllib.request.urlopen(request) as response:
                body = json.load(response)
            self.assertTrue(body["ok"])
            self.assertEqual(body["to"], "done")
            tasks = {
                task["id"]: task
                for milestone in body["data"]["milestones"]
                for task in milestone["tasks"]
            }
            self.assertEqual(tasks["m0-alpha"]["status"], "done")
            self.assertEqual(tasks["m0-alpha"]["source"], "manual")

            with urllib.request.urlopen(base + "/") as response:
                html = response.read().decode("utf-8")
            self.assertIn("const BOOTSTRAP = {", html)
            self.assertIn("Elocutio", html)
        finally:
            self.stop_server(server, thread)

    def test_errors_are_json(self) -> None:
        server, thread, base = self.start_server()
        try:
            with self.assertRaises(urllib.error.HTTPError) as context:
                urllib.request.urlopen(base + "/api/tasks/does-not-exist")
            self.assertEqual(context.exception.code, 404)

            request = urllib.request.Request(
                base + "/api/tasks/does-not-exist",
                data=json.dumps({"status": "done"}).encode("utf-8"),
                headers={"Content-Type": "application/json"},
                method="PATCH",
            )
            with self.assertRaises(urllib.error.HTTPError) as context:
                urllib.request.urlopen(request)
            self.assertEqual(context.exception.code, 404)
            body = json.loads(context.exception.read().decode("utf-8"))
            self.assertFalse(body["ok"])
        finally:
            self.stop_server(server, thread)


if __name__ == "__main__":
    unittest.main()
