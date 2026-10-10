#!/usr/bin/env python3
"""Check tracked Markdown headings, local links and the scripts index."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote


REPOSITORY = Path(__file__).resolve().parent.parent
MARKDOWN_LINK = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
SCRIPTS = REPOSITORY / "scripts"
SCRIPTS_INDEX = SCRIPTS / "README.md"


def tracked_markdown_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "*.md"],
        cwd=REPOSITORY,
        check=True,
        capture_output=True,
        text=True,
    )
    tracked = [
        REPOSITORY / line
        for line in result.stdout.splitlines()
        if line and (REPOSITORY / line).exists()
    ]
    return sorted(set(tracked))


def link_path(raw_target: str) -> str | None:
    target = raw_target.strip()
    if target.startswith("<") and target.endswith(">"):
        target = target[1:-1]
    if not target or target.startswith(("#", "/", "http://", "https://", "mailto:", "data:")):
        return None
    if " " in target:
        target = target.split(" ", 1)[0]
    target = target.split("#", 1)[0].split("?", 1)[0]
    return unquote(target) or None


def script_index_failures() -> tuple[list[str], int]:
    """Every file in scripts/ must be listed in its index, scripts/README.md.

    The testing guide points at this index for per-script coverage; a script that
    exists but is not indexed is how coverage gets lost without anyone noticing.
    """
    index = SCRIPTS_INDEX.read_text(encoding="utf-8")
    scripts = sorted(
        path for path in SCRIPTS.iterdir() if path.is_file() and path.name != SCRIPTS_INDEX.name
    )
    failures = [
        f"scripts/README.md: missing index entry for scripts/{script.name}"
        for script in scripts
        if f"`{script.name}`" not in index
    ]
    return failures, len(scripts)


def main() -> int:
    failures: list[str] = []
    files = tracked_markdown_files()
    index_failures, indexed_scripts = script_index_failures()
    failures.extend(index_failures)
    for markdown in files:
        text = markdown.read_text(encoding="utf-8")
        headings = [line for line in text.splitlines() if line.startswith("# ")]
        relative_name = markdown.relative_to(REPOSITORY)
        if len(headings) != 1:
            failures.append(f"{relative_name}: expected one H1, found {len(headings)}")
        for match in MARKDOWN_LINK.finditer(text):
            target = link_path(match.group(1))
            if target is None:
                continue
            resolved = (markdown.parent / target).resolve()
            try:
                resolved.relative_to(REPOSITORY)
            except ValueError:
                failures.append(f"{relative_name}: local link escapes repository: {target}")
                continue
            if not resolved.exists():
                failures.append(f"{relative_name}: missing local link target: {target}")

    if failures:
        print("documentation check failed", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"documentation check passed ({len(files)} Markdown files, {indexed_scripts} indexed scripts)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
