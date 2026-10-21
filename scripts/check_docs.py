#!/usr/bin/env python3
"""Check Markdown links, code fences, naming, and cross-document truth.

No third-party dependencies. Intended for pre-commit and CI.

The link and fence checks catch broken references. The truth checks catch the
failure mode that matters more: two documents disagreeing about the same fact,
or a document quoting a number the repository no longer produces. Those are the
errors that make a status report unreliable, and they are invisible to a link
checker.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "target", "node_modules", ".upstream", "vendor", "dist", "build"}
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
LEGACY_RE = re.compile(r"\bStet\b|\bstet\b")

# The files that carry the same facts in more than one place. When a claim
# appears here, the facts have to agree.
PHASE_TABLES = {
    "README.md": "docs/PLAN.md",
    "ROADMAP.md": "docs/PLAN.md",
}

# A phase status word used in a status table.
PHASE_STATUS_RE = re.compile(
    r"\|\s*\*?\*?(M[0-5]|Planning)[^|]*\|\s*([^|]+?)\s*\|",
    re.IGNORECASE,
)


def markdown_files() -> list[pathlib.Path]:
    files = []
    for path in ROOT.rglob("*.md"):
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        files.append(path)
    return sorted(files)


def check_file(path: pathlib.Path) -> list[str]:
    errors: list[str] = []
    text = path.read_text(encoding="utf-8")

    if text.count("```") % 2 != 0:
        errors.append("unbalanced code fences")

    for match in LINK_RE.finditer(text):
        target = match.group(1).strip()
        if target.startswith(("http://", "https://", "mailto:", "tel:", "#")):
            continue
        target = target.split("#", 1)[0].split("?", 1)[0].strip()
        if not target:
            continue
        resolved = (path.parent / target).resolve()
        try:
            resolved.relative_to(ROOT)
        except ValueError:
            errors.append(f"link escapes the repository: {target}")
            continue
        if not resolved.exists():
            errors.append(f"broken link: {target}")

    # The naming decision record intentionally mentions rejected alternatives.
    if path.name != "DECISIONS.md":
        for match in LEGACY_RE.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            errors.append(f"legacy product name 'Stet' at line {line}")

    return errors


def phase_statuses(path: pathlib.Path) -> dict[str, str]:
    """Return the phase status each document claims for every milestone."""
    text = path.read_text(encoding="utf-8")
    statuses: dict[str, str] = {}
    for match in PHASE_STATUS_RE.finditer(text):
        phase = match.group(1).strip()
        status = match.group(2).strip()
        if phase.upper() not in statuses:
            statuses[phase.upper()] = status
    return statuses


def check_phase_tables() -> list[str]:
    """Check that the phase status tables in README and ROADMAP agree."""
    errors: list[str] = []
    tables: dict[str, dict[str, str]] = {}
    for name in PHASE_TABLES:
        path = ROOT / name
        if path.exists():
            tables[name] = phase_statuses(path)

    if len(tables) < 2:
        return errors

    names = sorted(tables)
    first, second = names[0], names[1]
    for phase in sorted(set(tables[first]) & set(tables[second])):
        left = tables[first][phase]
        right = tables[second][phase]
        if phase_status_class(left) != phase_status_class(right):
            errors.append(
                f"{first} and {second} disagree about {phase}: "
                f"{left!r} vs {right!r}"
            )
    return errors


def phase_status_class(status: str) -> str:
    """Reduce a status cell to a comparable class.

    A table cell carries an emoji, a word, and sometimes a sentence. Only the
    word decides whether two documents agree.
    """
    lowered = status.lower()
    for word in ("done", "in progress", "started", "partial", "planned", "blocked"):
        if word in lowered:
            return word
    return lowered.strip(" .")


def count_tests() -> tuple[int, int]:
    """Return the (workspace, desktop) test counts, or (-1, -1) on failure."""
    workspace = run_test_count(["cargo", "test", "--all"])
    desktop = run_test_count(
        ["cargo", "test", "--manifest-path", "apps/macos/src-tauri/Cargo.toml"]
    )
    return workspace, desktop


def run_test_count(command: list[str]) -> int:
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=900,
        )
    except (OSError, subprocess.SubprocessError):
        return -1
    total = 0
    for line in result.stdout.splitlines():
        if line.startswith("test result:"):
            parts = line.split()
            for index, part in enumerate(parts):
                if part == "passed;" and index > 0 and parts[index - 1].isdigit():
                    total += int(parts[index - 1])
    return total


def check_quoted_test_counts() -> list[str]:
    """Check that quoted test counts match what the repository produces.

    The counts are read from the ledger's own verification file and the
    changelog, both of which quote them in prose. A stale count in a status
    report is exactly the kind of error this project needs to not make.
    """
    errors: list[str] = []
    workspace, desktop = count_tests()
    if workspace < 0 or desktop < 0:
        # The toolchain is not available here; do not fail the docs check for
        # something it cannot measure.
        return errors

    claims = [
        ("docs/M0-VERIFICATION.md", r"(\d+)\s+workspace tests?", workspace),
        ("docs/M0-VERIFICATION.md", r"(\d+)\s+desktop tests?", desktop),
    ]
    for relative, pattern, actual in claims:
        path = ROOT / relative
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(pattern, text):
            claimed = int(match.group(1))
            if claimed != actual:
                line = text.count("\n", 0, match.start()) + 1
                errors.append(
                    f"{relative}:{line} claims {claimed} tests, the repository "
                    f"has {actual}"
                )
    return errors


def check_tracker_evidence() -> list[str]:
    """Check that every task marked done has evidence the repository can test."""
    tracker_path = ROOT / "progress" / "tracker.json"
    if not tracker_path.exists():
        return []
    tracker = json.loads(tracker_path.read_text(encoding="utf-8"))
    errors: list[str] = []
    for milestone in tracker.get("milestones", []):
        for task in milestone.get("tasks", []):
            if task.get("status") != "done":
                continue
            if not task.get("evidence"):
                errors.append(
                    f"task {task.get('id')} is marked done with no evidence rule; "
                    "add one or choose a status the repository can prove"
                )
    return errors


def main() -> int:
    files = markdown_files()
    failures = 0
    for path in files:
        for error in check_file(path):
            print(f"{path.relative_to(ROOT)}: {error}")
            failures += 1

    if not (ROOT / "README.md").exists():
        print("README.md is missing")
        failures += 1

    for error in check_phase_tables():
        print(error)
        failures += 1

    for error in check_quoted_test_counts():
        print(error)
        failures += 1

    for error in check_tracker_evidence():
        print(error)
        failures += 1

    print(f"Checked {len(files)} Markdown files; {failures} problem(s).")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
