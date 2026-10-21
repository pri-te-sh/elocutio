#!/usr/bin/env python3
"""Elocutio build-progress tracker and live dashboard.

The single source of truth is ``progress/tracker.json``: milestones, work
items, estimates, acceptance criteria, blockers, and risks. Task status is
*derived* from repository evidence (files, text, globs, git remotes or commit
subjects) and can be overridden by hand. This script computes the state,
renders ``progress/template.html``, and can serve a live dashboard that
re-reads the repository on every request and writes manual status changes
back to the ledger.

Commands
--------
    build     Write a static snapshot to progress/out/
    serve     Serve the live dashboard on 127.0.0.1
    validate  Check the ledger for schema and consistency errors (CI)
    status    Set or clear a task's manual status
    summary   Print a compact terminal summary

The script uses the Python standard library only (3.11+).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import threading
import time
import webbrowser
from datetime import date, datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

REPO_ROOT = Path(__file__).resolve().parent.parent
TRACKER_REL = Path("progress/tracker.json")
TEMPLATE_REL = Path("progress/template.html")
OUTPUT_REL = Path("progress/out")

SCHEMA_VERSION = 1
STATUS_VALUES = ("planned", "in_progress", "done", "blocked")
STATUS_ORDER = {status: index for index, status in enumerate(STATUS_VALUES)}
BOOTSTRAP_MARKER = "/*__BOOTSTRAP__*/ null"
DEFAULT_PORT = 8787
DEFAULT_REFRESH_SECONDS = 5
MAX_REQUEST_BYTES = 64 * 1024
MAX_TEXT_EVIDENCE_BYTES = 5 * 1024 * 1024
TASK_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")


class ProgressError(Exception):
    """A user-facing tracker error."""


class UnknownTaskError(ProgressError):
    """Raised when a task id does not exist in the ledger."""


# ---------------------------------------------------------------------------
# Time and small helpers
# ---------------------------------------------------------------------------


def utc_now() -> str:
    """Return the current UTC time as an ISO 8601 string with a Z suffix."""
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def today() -> date:
    """Return the local calendar date."""
    return datetime.now().astimezone().date()


def clamp(value: float, low: float, high: float) -> float:
    """Clamp a number to an inclusive range."""
    return max(low, min(high, value))


def midpoint(estimate: list[int]) -> float:
    """Return the midpoint of a two-element effort estimate in days."""
    return (float(estimate[0]) + float(estimate[1])) / 2.0


def is_within(path: Path, root: Path) -> bool:
    """Return True when a resolved path stays inside the repository root."""
    try:
        path.resolve().relative_to(root.resolve())
    except ValueError:
        return False
    return True


def read_json(path: Path) -> dict:
    """Read a JSON object from disk with a helpful error message."""
    try:
        raw = path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ProgressError(f"{path} does not exist") from exc
    except OSError as exc:
        raise ProgressError(f"cannot read {path}: {exc}") from exc
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ProgressError(f"{path} is not valid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ProgressError(f"{path} must contain a JSON object")
    return value


def load_tracker(root: Path) -> dict:
    """Load the tracker ledger."""
    return read_json(root / TRACKER_REL)


def save_tracker(root: Path, tracker: dict) -> None:
    """Atomically write the tracker ledger with a fresh timestamp."""
    tracker["updatedAt"] = utc_now()
    path = root / TRACKER_REL
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(
        json.dumps(tracker, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def load_template(root: Path) -> str:
    """Load the dashboard HTML template."""
    path = root / TEMPLATE_REL
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ProgressError(f"{path} does not exist") from exc


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------


def _is_relative_selector(value: str) -> bool:
    """Return True for repository-relative paths and glob patterns."""
    text = value.strip()
    if not text or text.startswith(("/", "\\")) or re.match(r"^[A-Za-z]:", text):
        return False
    return ".." not in Path(text).parts


def validate_tracker(tracker: dict, root: Path | None = None) -> list[str]:
    """Return a list of human-readable schema and consistency errors."""
    errors: list[str] = []
    repo_root = root if root is not None else REPO_ROOT

    if tracker.get("schemaVersion") != SCHEMA_VERSION:
        errors.append(f"schemaVersion must be {SCHEMA_VERSION}")

    project = tracker.get("project")
    if not isinstance(project, dict):
        errors.append("project must be an object")
        project = {}
    for key in ("name", "tagline", "status", "startDate", "repo", "plan", "roadmap"):
        if not isinstance(project.get(key), str) or not project.get(key):
            errors.append(f"project.{key} must be a non-empty string")
    try:
        date.fromisoformat(str(project.get("startDate", "")))
    except ValueError:
        errors.append("project.startDate must be an ISO date (YYYY-MM-DD)")
    if isinstance(project.get("repo"), str) and not re.fullmatch(
        r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", project["repo"]
    ):
        errors.append("project.repo must look like owner/name")
    plan_weeks = project.get("planWeeks")
    if not (
        isinstance(plan_weeks, list)
        and len(plan_weeks) == 2
        and all(isinstance(value, int) and not isinstance(value, bool) for value in plan_weeks)
    ):
        errors.append("project.planWeeks must be [min, max] integers")
    elif plan_weeks[0] < 0 or plan_weeks[0] > plan_weeks[1]:
        errors.append("project.planWeeks must satisfy 0 <= min <= max")

    areas = tracker.get("areas")
    if not isinstance(areas, dict) or not areas:
        errors.append("areas must be a non-empty object")
        areas = {}
    priorities = tracker.get("priorities")
    if not isinstance(priorities, dict) or not priorities:
        errors.append("priorities must be a non-empty object")
        priorities = {}

    milestones = tracker.get("milestones")
    if not isinstance(milestones, list) or not milestones:
        errors.append("milestones must be a non-empty list")
        milestones = []

    seen_milestones: set[str] = set()
    seen_tasks: set[str] = set()
    task_ids: list[str] = []

    for milestone in milestones:
        if not isinstance(milestone, dict):
            errors.append("milestone entries must be objects")
            continue
        mid = milestone.get("id")
        if not isinstance(mid, str) or not mid:
            errors.append("milestone is missing an id")
            continue
        if mid in seen_milestones:
            errors.append(f"duplicate milestone id: {mid}")
        seen_milestones.add(mid)

        for key in ("name", "short", "outcome"):
            if not isinstance(milestone.get(key), str) or not milestone.get(key):
                errors.append(f"{mid}: {key} must be a non-empty string")

        estimate = milestone.get("estimateDays")
        if not (isinstance(estimate, list) and len(estimate) == 2 and all(isinstance(v, int) for v in estimate)):
            errors.append(f"{mid}: estimateDays must be [min, max] integers")
            estimate = [0, 0]
        elif estimate[0] > estimate[1] or estimate[0] < 0:
            errors.append(f"{mid}: estimateDays must satisfy 0 <= min <= max")

        weeks = milestone.get("weeks")
        if not (isinstance(weeks, list) and len(weeks) == 2 and all(isinstance(v, int) for v in weeks)):
            errors.append(f"{mid}: weeks must be [start, end] integers")
            weeks = [1, 1]
        elif weeks[0] > weeks[1] or weeks[0] < 0:
            errors.append(f"{mid}: weeks must satisfy 0 <= start <= end")

        if "status" in milestone and milestone["status"] not in STATUS_VALUES:
            errors.append(f"{mid}: unknown milestone status {milestone['status']!r}")
        if "completedAt" in milestone:
            try:
                date.fromisoformat(str(milestone["completedAt"]))
            except ValueError:
                errors.append(f"{mid}: completedAt must be an ISO date")

        tasks = milestone.get("tasks")
        if not isinstance(tasks, list) or not tasks:
            errors.append(f"{mid}: tasks must be a non-empty list")
            tasks = []
        milestone_task_ids: set[str] = set()
        for task in tasks:
            if not isinstance(task, dict):
                errors.append(f"{mid}: task entries must be objects")
                continue
            tid = task.get("id")
            if not isinstance(tid, str) or not TASK_ID_RE.match(tid):
                errors.append(f"{mid}: invalid task id {tid!r}")
                continue
            if tid in seen_tasks:
                errors.append(f"duplicate task id: {tid}")
            seen_tasks.add(tid)
            milestone_task_ids.add(tid)
            task_ids.append(tid)
            if not isinstance(task.get("title"), str) or not task.get("title"):
                errors.append(f"{tid}: title must be a non-empty string")
            if task.get("area") not in areas:
                errors.append(f"{tid}: area {task.get('area')!r} is not declared in areas")
            if task.get("priority") not in priorities:
                errors.append(f"{tid}: priority {task.get('priority')!r} is not declared in priorities")
            estimate_days = task.get("estimateDays", 1)
            if not isinstance(estimate_days, (int, float)) or estimate_days < 0:
                errors.append(f"{tid}: estimateDays must be a non-negative number")
            if "status" in task and task["status"] not in STATUS_VALUES:
                errors.append(f"{tid}: unknown status {task['status']!r}")
            if "notes" in task and not isinstance(task["notes"], str):
                errors.append(f"{tid}: notes must be a string")
            evidence = task.get("evidence", [])
            if not isinstance(evidence, list):
                errors.append(f"{tid}: evidence must be a list")
                evidence = []
            for rule in evidence:
                if not isinstance(rule, dict) or rule.get("type") not in (
                    "path",
                    "glob",
                    "text",
                    "git-remote",
                    "git-commit",
                ):
                    errors.append(f"{tid}: unsupported evidence rule {rule!r}")
                    continue
                if rule["type"] in ("path", "text") and not isinstance(rule.get("path"), str):
                    errors.append(f"{tid}: {rule['type']} evidence needs a path")
                elif rule["type"] in ("path", "text") and not _is_relative_selector(rule["path"]):
                    errors.append(f"{tid}: {rule['type']} evidence path must stay relative to the repository")
                if rule["type"] == "text" and not isinstance(rule.get("contains"), str):
                    errors.append(f"{tid}: text evidence needs a contains string")
                if rule["type"] == "glob" and not isinstance(rule.get("pattern"), str):
                    errors.append(f"{tid}: glob evidence needs a pattern")
                elif rule["type"] == "glob" and not _is_relative_selector(rule["pattern"]):
                    errors.append(f"{tid}: glob evidence pattern must stay relative to the repository")
                if rule["type"] in ("git-remote", "git-commit") and not isinstance(
                    rule.get("name" if rule["type"] == "git-remote" else "pattern"), str
                ):
                    errors.append(f"{tid}: {rule['type']} evidence is missing its selector")
                if rule["type"] == "glob" and isinstance(rule.get("pattern"), str):
                    # A glob that matches nothing can never pass, so it is not a
                    # check — it is a typo. Catch it in `validate` instead of
                    # letting it sit silently in the dashboard forever.
                    pattern = rule["pattern"]
                    if _is_relative_selector(pattern):
                        matches = [
                            candidate
                            for candidate in repo_root.glob(pattern)
                            if candidate.is_file()
                        ]
                        if not matches:
                            errors.append(
                                f"{tid}: glob evidence pattern matches no file: {pattern}"
                            )
            depends_on = task.get("dependsOn", [])
            if not isinstance(depends_on, list) or any(not isinstance(dep, str) for dep in depends_on):
                errors.append(f"{tid}: dependsOn must be a list of task ids")

        acceptance = milestone.get("acceptance", [])
        if not isinstance(acceptance, list) or not acceptance:
            errors.append(f"{mid}: acceptance must be a non-empty list")
            acceptance = []
        for item in acceptance:
            if not isinstance(item, dict) or not isinstance(item.get("text"), str):
                errors.append(f"{mid}: acceptance items must have text")
            elif not isinstance(item.get("accepted"), bool):
                errors.append(f"{mid}: acceptance item {item.get('text')!r} needs a boolean accepted")

    known_tasks = set(task_ids)
    for milestone in milestones:
        for task in milestone.get("tasks", []) if isinstance(milestone, dict) else []:
            for dependency in task.get("dependsOn", []) if isinstance(task, dict) else []:
                if dependency not in known_tasks:
                    errors.append(f"{task.get('id')}: dependency {dependency!r} does not exist")
                if dependency == task.get("id"):
                    errors.append(f"{task.get('id')}: a task cannot depend on itself")

    blockers = tracker.get("blockers", [])
    if not isinstance(blockers, list):
        errors.append("blockers must be a list")
    else:
        for blocker in blockers:
            if not isinstance(blocker, dict) or not isinstance(blocker.get("title"), str):
                errors.append("blocker entries must have a title")
            if isinstance(blocker, dict) and blocker.get("task") and blocker["task"] not in known_tasks:
                errors.append(f"blocker {blocker.get('title')!r} references unknown task {blocker['task']!r}")

    risks = tracker.get("risks", [])
    if not isinstance(risks, list):
        errors.append("risks must be a list")
    else:
        for risk in risks:
            if not isinstance(risk, dict) or not isinstance(risk.get("risk"), str):
                errors.append("risk entries must have a risk string")

    history = tracker.get("history", [])
    if not isinstance(history, list):
        errors.append("history must be a list")
    else:
        for entry in history:
            if not isinstance(entry, dict) or not isinstance(entry.get("task"), str):
                errors.append("history entries must reference a task")
            elif entry.get("task") not in known_tasks:
                errors.append(f"history references unknown task {entry['task']!r}")

    return errors


# ---------------------------------------------------------------------------
# Repository evidence
# ---------------------------------------------------------------------------


def _run_git(root: Path, *args: str, timeout: float = 5.0) -> str:
    """Run a git command and return stripped stdout, or '' on any failure."""
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


_GIT_CACHE: dict[str, tuple[float, dict]] = {}
_GIT_CACHE_LOCK = threading.Lock()
_GIT_CACHE_SECONDS = 2.5


def git_context(root: Path) -> dict:
    """Collect git facts, with a short cache to keep polling cheap."""
    key = str(root.resolve())
    now = time.monotonic()
    with _GIT_CACHE_LOCK:
        cached = _GIT_CACHE.get(key)
    if cached is not None and now - cached[0] < _GIT_CACHE_SECONDS:
        return cached[1]
    context = _collect_git_context(root)
    with _GIT_CACHE_LOCK:
        _GIT_CACHE[key] = (now, context)
    return context


def _collect_git_context(root: Path) -> dict:
    """Collect the git facts used by the dashboard and by evidence rules."""
    context = {
        "available": False,
        "branch": None,
        "head": None,
        "headSubject": None,
        "headAt": None,
        "totalCommits": 0,
        "dirtyCount": 0,
        "dirtyFiles": [],
        "remotes": [],
        "recent": [],
        "byDay": [],
        "weekCommits": 0,
        "subjects": [],
    }
    if not (root / ".git").exists():
        return context

    context["available"] = True
    context["branch"] = _run_git(root, "branch", "--show-current") or None
    context["head"] = _run_git(root, "rev-parse", "--short", "HEAD") or None
    count = _run_git(root, "rev-list", "--count", "HEAD")
    context["totalCommits"] = int(count) if count.isdigit() else 0
    context["remotes"] = [line for line in _run_git(root, "remote").splitlines() if line.strip()]

    status_lines = [line for line in _run_git(root, "status", "--porcelain").splitlines() if line]
    context["dirtyCount"] = len(status_lines)
    context["dirtyFiles"] = [line[3:] for line in status_lines[:10]]

    log_format = "%h%x1f%ad%x1f%s"
    for line in _run_git(
        root, "log", "-n", "12", "--date=iso-strict", f"--pretty=format:{log_format}"
    ).splitlines():
        parts = line.split("\x1f")
        if len(parts) != 3:
            continue
        context["recent"].append({"sha": parts[0], "at": parts[1], "subject": parts[2]})
    if context["recent"]:
        context["headSubject"] = context["recent"][0]["subject"]
        context["headAt"] = context["recent"][0]["at"]

    subjects = _run_git(root, "log", "-n", "400", "--pretty=format:%s").splitlines()
    context["subjects"] = [subject for subject in subjects if subject]

    week_start = today() - timedelta(days=6)
    counts: dict[str, int] = {}
    for stamp in _run_git(root, "log", "--since=13 days ago", "--date=short", "--pretty=format:%ad").splitlines():
        stamp = stamp.strip()
        if stamp:
            counts[stamp] = counts.get(stamp, 0) + 1
    by_day = []
    for offset in range(13, -1, -1):
        day = today() - timedelta(days=offset)
        key = day.isoformat()
        count_today = counts.get(key, 0)
        by_day.append({"date": key, "count": count_today})
        if day >= week_start:
            context["weekCommits"] += count_today
    context["byDay"] = by_day

    return context


def evaluate_evidence(rule: dict, root: Path, context: dict) -> dict:
    """Evaluate one evidence rule and return ``{label, ok, detail}``."""
    kind = rule.get("type")
    label = ""
    detail = ""
    ok = False

    if kind == "path":
        raw_path = str(rule.get("path", ""))
        path = root / raw_path
        label = str(rule.get("label") or raw_path)
        if not is_within(path, root):
            detail = "path escapes the repository"
        else:
            ok = path.exists()
            detail = "file present" if path.is_file() else ("directory present" if ok else "not found")
    elif kind == "glob":
        pattern = str(rule.get("pattern", ""))
        label = str(rule.get("label") or pattern)
        matches = [candidate for candidate in root.glob(pattern) if candidate.is_file()]
        matches = [candidate for candidate in matches if is_within(candidate, root)]
        ok = bool(matches)
        detail = f"{len(matches)} matching file(s)"
    elif kind == "text":
        raw_path = str(rule.get("path", ""))
        needle = str(rule.get("contains", ""))
        label = str(rule.get("label") or f"{raw_path}: {needle}")
        path = root / raw_path
        if not is_within(path, root):
            detail = "path escapes the repository"
        elif not path.is_file():
            detail = "file not found"
        elif path.stat().st_size > MAX_TEXT_EVIDENCE_BYTES:
            detail = "file is too large to check"
        else:
            try:
                contents = path.read_text(encoding="utf-8", errors="replace")
            except OSError as exc:
                detail = f"cannot read file: {exc}"
            else:
                ok = needle in contents
                detail = "marker present" if ok else "marker missing"
    elif kind == "git-remote":
        name = str(rule.get("name", ""))
        label = f"git remote {name}"
        ok = name in context.get("remotes", [])
        detail = "remote configured" if ok else "remote not configured"
    elif kind == "git-commit":
        pattern = str(rule.get("pattern", ""))
        label = f"commit subject ~ /{pattern}/"
        try:
            matcher = re.compile(pattern)
        except re.error as exc:
            detail = f"invalid regular expression: {exc}"
        else:
            ok = any(matcher.search(subject) for subject in context.get("subjects", []))
            detail = "matching commit found" if ok else "no matching commit"
    else:
        label = str(rule)
        detail = "unsupported evidence type"

    return {"label": label, "ok": ok, "detail": detail}


# ---------------------------------------------------------------------------
# Status, schedule, and payload computation
# ---------------------------------------------------------------------------


def derive_status(task: dict, evidence: list[dict]) -> str:
    """Derive a task status from its evidence rules."""
    if not evidence:
        return "planned"
    passed = sum(1 for item in evidence if item["ok"])
    if passed == len(evidence):
        return "done"
    if passed:
        return "in_progress"
    return "planned"


def task_payload(
    task: dict,
    milestone_id: str,
    tracker: dict,
    evidence: list[dict],
    waiting: list[tuple[str, str, str]],
) -> dict:
    """Build the computed payload for one work item."""
    suggested = derive_status(task, evidence)
    manual = task.get("status")
    status = manual if manual in STATUS_VALUES else suggested
    source = "manual" if manual in STATUS_VALUES else ("evidence" if evidence else "none")
    drift = bool(manual in STATUS_VALUES and manual != suggested and suggested == "done")
    # Drift only catches a manual status that lags behind the evidence. A manual
    # status that runs *ahead* of the evidence is the more dangerous case: a task
    # marked done with no evidence at all, or with failing evidence. It has to be
    # visible too, otherwise the dashboard green-lights exactly the claims that
    # nothing verifies.
    unsupported = bool(
        manual in STATUS_VALUES
        and manual in ("done", "blocked")
        and suggested != manual
    )
    unverified = bool(manual in STATUS_VALUES and manual == "done" and not evidence)

    payload = {
        "id": task["id"],
        "milestoneId": milestone_id,
        "title": task["title"],
        "area": task["area"],
        "areaLabel": tracker.get("areas", {}).get(task["area"], task["area"]),
        "priority": task["priority"],
        "priorityLabel": tracker.get("priorities", {}).get(task["priority"], task["priority"]),
        "estimateDays": task.get("estimateDays", 1),
        "status": status,
        "source": source,
        "suggestedStatus": suggested,
        "drift": drift,
        "unsupported": unsupported,
        "unverified": unverified,
        "manualStatus": manual if manual in STATUS_VALUES else None,
        "evidence": evidence,
        "evidencePassed": sum(1 for item in evidence if item["ok"]),
        "evidenceTotal": len(evidence),
        "evidenceComplete": bool(evidence) and all(item["ok"] for item in evidence),
        "notes": task.get("notes", ""),
        "dependsOn": task.get("dependsOn", []),
        "waitingOn": [
            {"id": dependency, "title": title, "status": status_value}
            for dependency, title, status_value in waiting
        ],
    }
    return payload


def planned_window(project: dict, milestone: dict) -> tuple[date, date]:
    """Return the planned start and end dates for a milestone."""
    start = date.fromisoformat(project["startDate"])
    weeks = milestone.get("weeks", [1, 1])
    start_week, end_week = int(weeks[0]), int(weeks[1])
    if start_week <= 0:
        return start, start
    window_start = start + timedelta(days=7 * (start_week - 1))
    window_end = start + timedelta(days=7 * end_week - 1)
    return window_start, window_end


def planned_fraction(window: tuple[date, date], on: date) -> float:
    """Return how much of a planned window has elapsed on a given date."""
    start, end = window
    if on < start:
        return 0.0
    if on >= end:
        return 1.0
    total = max(1, (end - start).days + 1)
    return clamp(((on - start).days + 1) / total, 0.0, 1.0)


def milestone_status(progress: float, tasks: list[dict], manual: str | None) -> str:
    """Derive a milestone status from its tasks, unless one is set by hand."""
    if manual in STATUS_VALUES:
        return manual
    if progress >= 1.0:
        return "done"
    if any(task["status"] in ("done", "in_progress", "blocked") for task in tasks):
        return "in_progress"
    return "planned"


def build_payload(
    root: Path,
    tracker: dict,
    mode: str = "live",
    refresh_seconds: int = DEFAULT_REFRESH_SECONDS,
) -> dict:
    """Compute the complete dashboard payload."""
    errors = validate_tracker(tracker, root)
    if errors:
        raise ProgressError("tracker is invalid:\n  - " + "\n  - ".join(errors))

    project = tracker["project"]
    git = git_context(root)
    now = today()
    start_date = date.fromisoformat(project["startDate"])

    # First pass: derive every task status so dependencies and milestone
    # progress can be computed consistently.
    statuses: dict[str, str] = {}
    titles: dict[str, str] = {}
    evidence_by_task: dict[str, list[dict]] = {}
    for milestone in tracker["milestones"]:
        for task in milestone["tasks"]:
            evidence = [evaluate_evidence(rule, root, git) for rule in task.get("evidence", [])]
            evidence_by_task[task["id"]] = evidence
            suggested = derive_status(task, evidence)
            manual = task.get("status")
            statuses[task["id"]] = manual if manual in STATUS_VALUES else suggested
            titles[task["id"]] = task["title"]

    milestone_payloads: list[dict] = []
    total_weight = 0.0
    actual_weighted = 0.0
    expected_weighted = 0.0
    remaining_min = 0.0
    remaining_max = 0.0
    task_counts = {"done": 0, "in_progress": 0, "planned": 0, "blocked": 0, "total": 0}
    milestone_done = 0
    current_milestone = None
    current_milestone_status = None

    for milestone in tracker["milestones"]:
        tasks: list[dict] = []
        for task in milestone["tasks"]:
            waiting = [
                (dependency, titles[dependency], statuses[dependency])
                for dependency in task.get("dependsOn", [])
                if statuses.get(dependency) != "done"
            ]
            tasks.append(
                task_payload(
                    task,
                    milestone["id"],
                    tracker,
                    evidence_by_task[task["id"]],
                    waiting,
                )
            )

        # Progress counts a finished task in full and a started task in part.
        # Counting only finished work made a milestone with six tasks in
        # progress read as 0%, which is the opposite of what the number is for.
        #
        # A started task earns credit from its evidence coverage, and a task
        # whose evidence all passes but which is still marked in progress earns
        # only half — the evidence shows the code landed, not that the work is
        # finished. Without that discount a task whose acceptance criteria are
        # still open would read as nearly complete.
        weighted_estimate = 0.0
        for task in tasks:
            estimate = task["estimateDays"]
            status = task["status"]
            if status == "done":
                weighted_estimate += estimate
            elif status == "in_progress":
                evidence = evidence_by_task.get(task["id"], [])
                if evidence:
                    passed = sum(1 for item in evidence if item["ok"])
                    weighted_estimate += estimate * (passed / len(evidence)) * 0.5
                else:
                    weighted_estimate += estimate * 0.25
        total_estimate = sum(task["estimateDays"] for task in tasks) or 1.0
        progress = clamp(weighted_estimate / total_estimate, 0.0, 1.0)
        status = milestone_status(progress, tasks, milestone.get("status"))
        counts = {key: 0 for key in ("done", "in_progress", "planned", "blocked")}
        for task in tasks:
            counts[task["status"]] = counts.get(task["status"], 0) + 1
        counts["total"] = len(tasks)
        for key, value in counts.items():
            if key != "total":
                task_counts[key] += value
        task_counts["total"] += len(tasks)
        if status == "done":
            milestone_done += 1
        if current_milestone is None and status != "done":
            current_milestone = milestone
            current_milestone_status = status

        window = planned_window(project, milestone)
        estimate = milestone["estimateDays"]
        weight = midpoint(estimate)
        if weight > 0:
            total_weight += weight
            actual_weighted += weight * progress
            expected_weighted += weight * planned_fraction(window, now)
            remaining_min += (1.0 - progress) * float(estimate[0])
            remaining_max += (1.0 - progress) * float(estimate[1])

        accepted = sum(1 for item in milestone.get("acceptance", []) if item["accepted"])
        milestone_payloads.append(
            {
                "id": milestone["id"],
                "name": milestone["name"],
                "short": milestone["short"],
                "outcome": milestone["outcome"],
                "status": status,
                "manualStatus": milestone.get("status") if milestone.get("status") in STATUS_VALUES else None,
                "progress": round(progress, 4),
                "estimateDays": estimate,
                "estimateMidpoint": weight,
                "weekLabel": milestone_week_label(milestone),
                "schedule": {
                    "start": window[0].isoformat(),
                    "end": window[1].isoformat(),
                    "plannedFraction": round(planned_fraction(window, now), 4),
                },
                "completedAt": milestone.get("completedAt"),
                "tasks": tasks,
                "counts": counts,
                "acceptance": milestone.get("acceptance", []),
                "acceptanceDone": accepted,
                "acceptanceTotal": len(milestone.get("acceptance", [])),
            }
        )

    overall = actual_weighted / total_weight if total_weight else 0.0
    expected = expected_weighted / total_weight if total_weight else 0.0
    plan_weeks = project.get("planWeeks", [0, 0])
    plan_start = start_date
    plan_end = start_date + timedelta(days=7 * int(plan_weeks[1]) - 1)
    projected_min = now + timedelta(days=round(remaining_min))
    projected_max = now + timedelta(days=round(remaining_max))

    blockers = []
    for blocker in tracker.get("blockers", []):
        entry = dict(blocker)
        entry["taskTitle"] = titles.get(blocker.get("task", ""), None)
        blockers.append(entry)

    history = []
    for entry in reversed(tracker.get("history", [])[-40:]):
        record = dict(entry)
        record["taskTitle"] = titles.get(record.get("task", ""), record.get("task"))
        history.append(record)

    tracker_hash = hashlib.sha256(
        json.dumps(tracker, sort_keys=True, ensure_ascii=False, default=str).encode("utf-8")
    ).hexdigest()[:16]

    payload = {
        "schemaVersion": SCHEMA_VERSION,
        "mode": mode,
        "generatedAt": utc_now(),
        "refreshSeconds": int(refresh_seconds),
        "project": {
            "name": project["name"],
            "tagline": project["tagline"],
            "description": project.get("description", ""),
            "status": project["status"],
            "startDate": project["startDate"],
            "repo": project["repo"],
            "repoUrl": f"https://github.com/{project['repo']}",
            "planPath": project["plan"],
            "roadmapPath": project["roadmap"],
            "planWeeks": plan_weeks,
        },
        "summary": {
            "overallProgress": round(overall, 4),
            "expectedProgress": round(expected, 4),
            "variance": round(overall - expected, 4),
            "onTrack": overall >= expected - 0.02,
            "phaseId": current_milestone["id"] if current_milestone else None,
            "phaseName": current_milestone["name"] if current_milestone else "Complete",
            "phaseStatus": current_milestone_status or "done",
            "tasks": task_counts,
            "milestonesDone": milestone_done,
            "milestonesTotal": len(tracker["milestones"]),
            "daysElapsed": max(0, (now - start_date).days) + 1,
            "planWindow": {"start": plan_start.isoformat(), "end": plan_end.isoformat()},
            "remainingDays": [round(remaining_min, 1), round(remaining_max, 1)],
            "projectedFinish": {
                "earliest": projected_min.isoformat(),
                "latest": projected_max.isoformat(),
            },
            "updatedAt": tracker.get("updatedAt"),
            "git": git,
        },
        "milestones": milestone_payloads,
        "blockers": blockers,
        "risks": tracker.get("risks", []),
        "history": history,
        "meta": {
            "areas": [{"id": key, "label": value} for key, value in tracker.get("areas", {}).items()],
            "priorities": tracker.get("priorities", {}),
            "trackerPath": str(TRACKER_REL),
            "planPath": project["plan"],
            "roadmapPath": project["roadmap"],
            "repoUrl": f"https://github.com/{project['repo']}",
        },
    }
    payload["meta"]["trackerHash"] = tracker_hash
    payload["rev"] = payload_revision(payload)
    return payload


def milestone_week_label(milestone: dict) -> str:
    """Return a human label for a milestone's planned weeks."""
    weeks = milestone.get("weeks", [1, 1])
    start_week, end_week = int(weeks[0]), int(weeks[1])
    if start_week <= 0:
        return "Foundation"
    if start_week == end_week:
        return f"Week {start_week}"
    return f"Weeks {start_week}–{end_week}"


def payload_revision(payload: dict) -> str:
    """Return a short fingerprint of everything that affects the dashboard."""
    fingerprint = {
        "updatedAt": payload["summary"]["updatedAt"],
        "tasks": [
            [
                task["id"],
                task["status"],
                task["source"],
                [item["ok"] for item in task["evidence"]],
            ]
            for milestone in payload["milestones"]
            for task in milestone["tasks"]
        ],
        "git": [
            payload["summary"]["git"]["head"],
            payload["summary"]["git"]["dirtyCount"],
            payload["summary"]["git"]["totalCommits"],
        ],
        "blockers": [blocker.get("id") for blocker in payload["blockers"]],
        "history": len(payload["history"]),
        "trackerHash": payload["meta"]["trackerHash"],
    }
    return hashlib.sha256(
        json.dumps(fingerprint, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()[:16]


# ---------------------------------------------------------------------------
# Rendering and static build
# ---------------------------------------------------------------------------


def render_html(template: str, payload: dict) -> str:
    """Inject the payload into the template."""
    if BOOTSTRAP_MARKER not in template:
        raise ProgressError(f"template is missing the {BOOTSTRAP_MARKER!r} marker")
    blob = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    blob = blob.replace("<", "\\u003c").replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
    return template.replace(BOOTSTRAP_MARKER, blob)


def build(root: Path, out_dir: Path) -> tuple[Path, Path]:
    """Write the static dashboard and its JSON payload."""
    tracker = load_tracker(root)
    payload = build_payload(root, tracker, mode="static", refresh_seconds=0)
    html = render_html(load_template(root), payload)
    out_dir.mkdir(parents=True, exist_ok=True)
    html_path = out_dir / "dashboard.html"
    json_path = out_dir / "progress.json"
    html_path.write_text(html, encoding="utf-8")
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return html_path, json_path


# ---------------------------------------------------------------------------
# Manual status edits
# ---------------------------------------------------------------------------


def find_task(tracker: dict, task_id: str) -> tuple[dict, dict]:
    """Return ``(milestone, task)`` for a task id, or raise."""
    for milestone in tracker.get("milestones", []):
        for task in milestone.get("tasks", []):
            if task.get("id") == task_id:
                return milestone, task
    raise UnknownTaskError(f"unknown task id: {task_id}")


def update_task_status(root: Path, task_id: str, status: str, note: str = "") -> tuple[str | None, str | None]:
    """Set or clear a manual status and record the change. Returns (old, new)."""
    if status != "auto" and status not in STATUS_VALUES:
        raise ProgressError(f"status must be one of {', '.join(STATUS_VALUES)} or 'auto'")
    tracker = load_tracker(root)
    _, task = find_task(tracker, task_id)
    previous = task.get("status") if task.get("status") in STATUS_VALUES else "auto"
    if status == "auto":
        task.pop("status", None)
    else:
        task["status"] = status
    tracker.setdefault("history", []).append(
        {
            "at": utc_now(),
            "task": task_id,
            "from": previous,
            "to": status,
            "note": note.strip(),
        }
    )
    tracker["history"] = tracker["history"][-500:]
    save_tracker(root, tracker)
    return (None if previous == "auto" else previous), (None if status == "auto" else status)


# ---------------------------------------------------------------------------
# Live server
# ---------------------------------------------------------------------------


def make_handler(root: Path, template: str, verbose: bool = False):
    """Create an HTTP handler bound to a repository root."""
    edit_lock = threading.Lock()

    class ProgressHandler(BaseHTTPRequestHandler):
        server_version = "ElocutioProgress/1.0"
        protocol_version = "HTTP/1.1"

        def log_message(self, format: str, *args) -> None:  # noqa: A002
            if verbose:
                sys.stderr.write(f"[progress] {self.address_string()} {format % args}\n")

        def _send(self, code: int, body: bytes, content_type: str) -> None:
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store, max-age=0")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(body)

        def _json(self, code: int, value: dict) -> None:
            self._send(code, json.dumps(value, ensure_ascii=False).encode("utf-8"), "application/json; charset=utf-8")

        def _html(self, code: int, text: str) -> None:
            self._send(code, text.encode("utf-8"), "text/html; charset=utf-8")

        def _payload(self) -> dict:
            return build_payload(root, load_tracker(root), mode="live", refresh_seconds=DEFAULT_REFRESH_SECONDS)

        def do_HEAD(self) -> None:  # noqa: N802
            self.do_GET()

        def do_GET(self) -> None:  # noqa: N802
            path = urlparse(self.path).path
            try:
                if path in ("/", "/index.html"):
                    payload = self._payload()
                    self._html(200, render_html(template, payload))
                elif path == "/api/progress":
                    self._json(200, self._payload())
                elif path == "/api/health":
                    tracker = load_tracker(root)
                    self._json(
                        200,
                        {
                            "ok": True,
                            "updatedAt": tracker.get("updatedAt"),
                            "generatedAt": utc_now(),
                        },
                    )
                elif path == "/favicon.ico":
                    self._send(204, b"", "image/x-icon")
                else:
                    self._json(404, {"ok": False, "error": f"no route for {path}"})
            except ProgressError as exc:
                self._json(500, {"ok": False, "error": str(exc)})
            except Exception as exc:  # pragma: no cover - defensive
                self._json(500, {"ok": False, "error": f"internal error: {exc}"})

        def do_PATCH(self) -> None:  # noqa: N802
            path = urlparse(self.path).path
            match = re.fullmatch(r"/api/tasks/([a-z0-9][a-z0-9-]{0,63})", path)
            if not match:
                self._json(404, {"ok": False, "error": f"no route for {path}"})
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                self._json(400, {"ok": False, "error": "invalid Content-Length"})
                return
            if length <= 0 or length > MAX_REQUEST_BYTES:
                if length > MAX_REQUEST_BYTES:
                    self.close_connection = True
                self._json(413 if length > MAX_REQUEST_BYTES else 400, {"ok": False, "error": "invalid request body"})
                return
            try:
                body = json.loads(self.rfile.read(length).decode("utf-8"))
            except (json.JSONDecodeError, UnicodeDecodeError):
                self._json(400, {"ok": False, "error": "body must be JSON"})
                return
            if not isinstance(body, dict):
                self._json(400, {"ok": False, "error": "body must be a JSON object"})
                return
            status = body.get("status")
            note = body.get("note", "")
            if not isinstance(status, str):
                self._json(400, {"ok": False, "error": "status is required"})
                return
            if not isinstance(note, str) or len(note) > 500:
                self._json(400, {"ok": False, "error": "note must be a string of at most 500 characters"})
                return
            try:
                with edit_lock:
                    old, new = update_task_status(root, match.group(1), status, note)
                    payload = self._payload()
            except UnknownTaskError as exc:
                self._json(404, {"ok": False, "error": str(exc)})
                return
            except ProgressError as exc:
                self._json(400, {"ok": False, "error": str(exc)})
                return
            self._json(200, {"ok": True, "task": match.group(1), "from": old, "to": new, "data": payload})

        def do_POST(self) -> None:  # noqa: N802
            self._json(405, {"ok": False, "error": "use PATCH /api/tasks/<id>"})

    return ProgressHandler


def serve(
    root: Path,
    host: str,
    port: int,
    should_open: bool,
    verbose: bool,
) -> int:
    """Serve the live dashboard until interrupted."""
    tracker = load_tracker(root)
    errors = validate_tracker(tracker, root)
    if errors:
        raise ProgressError("tracker is invalid:\n  - " + "\n  - ".join(errors))
    template = load_template(root)
    handler = make_handler(root, template, verbose=verbose)

    server = None
    last_error: OSError | None = None
    for candidate in range(port, port + 20):
        try:
            server = ThreadingHTTPServer((host, candidate), handler)
            server.daemon_threads = True
            break
        except OSError as exc:
            last_error = exc
            continue
    if server is None:
        raise ProgressError(f"could not bind {host}:{port}–{port + 19}: {last_error}")

    actual_port = server.server_address[1]
    url = f"http://{host}:{actual_port}/"
    print(f"Elocutio progress dashboard → {url}", flush=True)
    print("Live data: statuses, evidence, and git activity refresh on every request.", flush=True)
    print("Press Ctrl+C to stop.", flush=True)
    if should_open:
        threading.Timer(0.4, lambda: webbrowser.open(url)).start()
    try:
        server.serve_forever(poll_interval=0.25)
    except KeyboardInterrupt:
        print("\nStopping dashboard.")
    finally:
        server.server_close()
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def progress_summary(tracker: dict, payload: dict) -> str:
    """Return a compact text summary for the terminal."""
    summary = payload["summary"]
    lines = [
        f"{payload['project']['name']} — {percent(summary['overallProgress'])} complete "
        f"(planned by today: {percent(summary['expectedProgress'])})",
        "",
    ]
    for milestone in payload["milestones"]:
        counts = milestone["counts"]
        lines.append(
            f"  {milestone['id']:<2} {milestone['name']:<32} "
            f"{milestone['status']:<11} {percent(milestone['progress']):>4}  "
            f"({counts['done']}/{counts['total']} tasks)"
        )
    tasks = summary["tasks"]
    lines += [
        "",
        f"  Tasks: {tasks['done']} done · {tasks['in_progress']} in progress · "
        f"{tasks['planned']} planned · {tasks['blocked']} blocked",
        f"  Phase: {summary['phaseId']} {summary['phaseName']} · "
        f"milestones {summary['milestonesDone']}/{summary['milestonesTotal']} complete",
        f"  Updated: {payload['generatedAt']}",
    ]
    if summary["git"]["available"]:
        lines.append(
            f"  Git: {summary['git']['branch'] or 'detached'} @ {summary['git']['head'] or '?'} · "
            f"{summary['git']['dirtyCount']} changed file(s) · {summary['git']['weekCommits']} commit(s) this week"
        )
    if payload["blockers"]:
        lines.append(f"  Blockers: {len(payload['blockers'])}")
    return "\n".join(lines)


def percent(value: float) -> str:
    """Format a 0–1 fraction as a percentage."""
    return f"{round(value * 100)}%"


def command_build(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    out_dir = (root / OUTPUT_REL) if not args.out else Path(args.out).resolve()
    html_path, json_path = build(root, out_dir)
    payload = read_json(json_path)
    print(progress_summary(load_tracker(root), payload))
    print()
    print(f"Dashboard: {html_path}")
    print(f"Payload:   {json_path}")
    return 0


def command_serve(args: argparse.Namespace) -> int:
    return serve(
        root=Path(args.root).resolve(),
        host=args.host,
        port=args.port,
        should_open=args.open,
        verbose=args.verbose,
    )


def command_validate(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    tracker = load_tracker(root)
    errors = validate_tracker(tracker, root)
    for error in errors:
        print(f"progress/tracker.json: {error}")
    if errors:
        print(f"Tracker invalid: {len(errors)} problem(s).")
        return 1
    tasks = sum(len(milestone["tasks"]) for milestone in tracker["milestones"])
    print(f"Tracker OK: {len(tracker['milestones'])} milestones, {tasks} tasks, schema v{tracker['schemaVersion']}.")
    return 0


def command_status(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    tracker = load_tracker(root)
    _, task = find_task(tracker, args.task)
    old, new = update_task_status(root, args.task, args.status, args.note or "")
    payload = build_payload(root, load_tracker(root), mode="cli")
    effective = next(
        item["status"]
        for milestone in payload["milestones"]
        for item in milestone["tasks"]
        if item["id"] == args.task
    )
    print(f"{args.task}: {old or 'auto'} → {new or 'auto'} (effective: {effective})")
    print(f"Scope progress: {percent(payload['summary']['overallProgress'])}")
    return 0


def command_summary(args: argparse.Namespace) -> int:
    root = Path(args.root).resolve()
    tracker = load_tracker(root)
    payload = build_payload(root, tracker, mode="cli")
    print(progress_summary(tracker, payload))
    return 0


def build_parser() -> argparse.ArgumentParser:
    """Build the command-line parser."""
    parser = argparse.ArgumentParser(
        prog="progress.py",
        description="Track Elocutio's build progress from repository evidence.",
    )
    parser.add_argument(
        "--root",
        default=str(REPO_ROOT),
        help="repository root (default: the repository containing this script)",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    build_parser_ = subparsers.add_parser("build", help="write a static dashboard snapshot")
    build_parser_.add_argument("--out", help="output directory (default: progress/out)")
    build_parser_.set_defaults(func=command_build)

    serve_parser = subparsers.add_parser("serve", help="serve the live dashboard")
    serve_parser.add_argument("--host", default="127.0.0.1", help="bind address (default: 127.0.0.1)")
    serve_parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"port (default: {DEFAULT_PORT})")
    serve_parser.add_argument("--open", action="store_true", help="open the dashboard in a browser")
    serve_parser.add_argument("--verbose", action="store_true", help="log every HTTP request")
    serve_parser.set_defaults(func=command_serve)

    validate_parser = subparsers.add_parser("validate", help="validate progress/tracker.json")
    validate_parser.set_defaults(func=command_validate)

    status_parser = subparsers.add_parser("status", help="set or clear a task's manual status")
    status_parser.add_argument("task", help="task id, for example m0-build")
    status_parser.add_argument(
        "status",
        choices=[*STATUS_VALUES, "auto"],
        help="manual status, or 'auto' to fall back to evidence",
    )
    status_parser.add_argument("--note", help="optional history note")
    status_parser.set_defaults(func=command_status)

    summary_parser = subparsers.add_parser("summary", help="print a terminal summary")
    summary_parser.set_defaults(func=command_summary)

    return parser


def main(argv: list[str] | None = None) -> int:
    """Run the CLI."""
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.func(args))
    except ProgressError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
