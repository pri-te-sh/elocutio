#!/usr/bin/env python3
"""Check Markdown links, code fences, and legacy naming.

No third-party dependencies. Intended for pre-commit and CI.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP_DIRS = {".git", "target", "node_modules", ".upstream", "dist", "build"}
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
LEGACY_RE = re.compile(r"\bStet\b|\bstet\b")


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

    print(f"Checked {len(files)} Markdown files; {failures} problem(s).")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
