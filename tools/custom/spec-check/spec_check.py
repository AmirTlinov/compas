#!/usr/bin/env python3
"""Spec/ADR gate check for P02.

Checks that required pre-implementation documents exist and include mandatory sections:
- Goal
- Non-goals
- Acceptance
- Edge-cases
- Rollback
"""

from __future__ import annotations

import argparse
import json
import hashlib
import re
from pathlib import Path
from typing import List

SPEC_PATH = Path("docs/spec/p02-spec-adr-gate.md")
ADR_PATH = Path("docs/adr/ADR-0001-spec-adr-gate.md")

REQUIRED_SECTIONS = [
    "Goal",
    "Non-goals",
    "Acceptance",
    "Edge-cases",
    "Rollback",
]


def _hash(path: Path) -> str:
    data = path.read_bytes()
    return hashlib.sha256(data).hexdigest()


def _read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _has_sections(text: str) -> List[str]:
    missing: List[str] = []
    for section in REQUIRED_SECTIONS:
        pattern = re.compile(rf"^#+\s*{re.escape(section)}\b", re.MULTILINE | re.IGNORECASE)
        if not pattern.search(text):
            missing.append(section)
    return missing


def _build_report(repo_root: Path, status: str, file_records: list[dict], extra: list[str]):
    return {
        "plugin": "p02",
        "status": status,
        "scope": "spec-adr-gate",
        "repo_root": str(repo_root),
        "files": file_records,
        "problems": extra,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".", help="Repository root")
    args = parser.parse_args()

    repo_root = Path(args.repo_root).expanduser().resolve()
    files = [SPEC_PATH, ADR_PATH]

    file_records = []
    problems: List[str] = []

    for rel in files:
        path = repo_root / rel
        if not path.exists():
            problems.append(f"missing file: {rel}")
            continue

        text = _read_text(path)
        missing_sections = _has_sections(text)
        file_records.append(
            {
                "path": str(path.relative_to(repo_root)),
                "sha256": _hash(path),
                "missing_sections": missing_sections,
            }
        )
        for sec in missing_sections:
            problems.append(f"{rel}: missing required section '{sec}'")

    if problems:
        report = _build_report(repo_root, "fail", file_records, problems)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return 1

    report = _build_report(repo_root, "pass", file_records, [])
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
