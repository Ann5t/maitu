#!/usr/bin/env python3
"""Fail when files that are about to be committed contain credentials.

The repository rule is that credentials, real `.env` files and keys never enter
Git (AGENTS.md). `.gitignore` only protects files that stay ignored: a key pasted
into a tracked file, or a secret file forced in with `git add -f`, still lands in
history. This check therefore looks at the file set reported by
`git ls-files --cached --others --exclude-standard`, so an ignored local file is
never reported while a secret that is about to be committed is.

Reported lines only name the file, the line and the rule, never the matched
value: the check output itself must stay safe to paste into a review or a log.

Usage: scripts/check-secrets.py [path ...]  (default: the repository file set)
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
MAX_BYTES = 2_000_000

SECRET_FILE_NAME = re.compile(
    r"(^|/)(?:\.env(?:\.local|\.production|\.development)?|"
    r"id_(?:rsa|dsa|ecdsa|ed25519)|credentials\.json|"
    r"[^/]*\.(?:pem|key|p12|pfx|jks))$",
)
EXAMPLE_FILE_NAME = re.compile(r"(^|/)\.env\.(?:example|secure\.example)$")

CONTENT_PATTERNS = (
    ("private key block", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("provider API key", re.compile(r"\bsk-[A-Za-z0-9_-]{20,}")),
    (
        "GitHub token",
        re.compile(r"\b(?:ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})"),
    ),
    ("AWS access key id", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("Slack token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}")),
    ("Google API key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("connection string with password", re.compile(r"://([^:@/\s]{1,64}):([^@/\s]{1,128})@([A-Za-z0-9._:\-]{1,64})")),
)

PLACEHOLDER_PASSWORD = re.compile(
    r"^(?:password|passwd|secret|token|example|sample|changeme|placeholder|test|dummy|redacted|"
    r"\$\{[^}]*\}|<[^>]*>|\*+|x+)$",
    re.IGNORECASE,
)
LOCAL_HOSTS = frozenset({"localhost", "127.0.0.1", "::1", "[::1]", "host.docker.internal"})


def repository_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=REPOSITORY,
        check=True,
        capture_output=True,
        text=True,
    )
    files = [
        REPOSITORY / line
        for line in result.stdout.splitlines()
        if line and (REPOSITORY / line).is_file()
    ]
    return sorted(set(files))


def given_files(arguments: list[str]) -> list[Path]:
    files = [Path(argument) if Path(argument).is_absolute() else REPOSITORY / argument for argument in arguments]
    return [path for path in files if path.is_file()]


def display_name(path: Path) -> str:
    try:
        name = str(path.resolve().relative_to(REPOSITORY))
    except ValueError:
        name = str(path)
    # Windows callers pass backslash paths; the rules below are written with "/".
    return name.replace("\\", "/")


def host_is_local(host: str) -> bool:
    """A loopback/`.local` host or a single-label Docker/compose service name."""
    if host in LOCAL_HOSTS or host.endswith(".local"):
        return True
    return "." not in host and ":" not in host


def inspect_text(text: str, name: str) -> list[str]:
    failures: list[str] = []
    for rule, pattern in CONTENT_PATTERNS:
        for match in pattern.finditer(text):
            if rule == "connection string with password":
                host = match.group(3).rsplit(":", 1)[0]
                if PLACEHOLDER_PASSWORD.match(match.group(2)) or host_is_local(host):
                    continue
            line = text.count("\n", 0, match.start()) + 1
            failures.append(f"{name}:{line}: {rule}")
    return failures


def main(arguments: list[str]) -> int:
    files = given_files(arguments) if arguments else repository_files()
    failures: list[str] = []
    inspected = 0
    for path in files:
        name = display_name(path)
        if SECRET_FILE_NAME.search("/" + name.lstrip("/")) and not EXAMPLE_FILE_NAME.search(
            "/" + name.lstrip("/")
        ):
            failures.append(f"{name}: tracked secret-bearing file name")
            continue
        try:
            if path.stat().st_size > MAX_BYTES:
                continue
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        inspected += 1
        failures.extend(inspect_text(text, name))

    if failures:
        print("credentials check failed", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"credentials check passed ({inspected} files)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))