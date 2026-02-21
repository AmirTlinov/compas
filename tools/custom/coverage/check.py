#!/usr/bin/env python3
"""P15 coverage non-regression guard.

Compares current coverage baseline JSON against baseline commit.
Fail-closed: malformed input, missing metadata, or parsing failures fail.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def fail(message: str, code: int = 2) -> None:
    print(message)
    sys.exit(code)


def read_json(path: Path) -> dict:
    if not path.is_file():
        fail(f"coverage file not found: {path}")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        fail(f"failed to read {path}: {exc}")
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON in {path}: {exc}")


def as_int(payload: dict, key: str) -> int:
    val = payload.get(key)
    if not isinstance(val, int):
        fail(f"invalid baseline field {key}: expected int, got {type(val)!r}")
    if val < 0:
        fail(f"invalid baseline field {key}: negative value {val}")
    return val


def git_show(path: str) -> dict:
    try:
        output = subprocess.check_output(
            ["git", "show", f"HEAD:{path}"],
            stderr=subprocess.STDOUT,
            text=True,
        )
    except subprocess.CalledProcessError:
        return {}
    try:
        return json.loads(output)
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON in HEAD:{path}: {exc}")


def load_fallback_baseline() -> dict:
    fallback_path = Path(".agents/mcp/compas/baselines/quality_snapshot.json")
    if not fallback_path.is_file():
        fail(
            "unable to read baseline: no commit baseline for coverage.json and "
            f"fallback {fallback_path} missing"
        )
    snapshot = read_json(fallback_path)
    return {
        "coverage_covered": as_int(snapshot, "coverage_covered"),
        "coverage_total": as_int(snapshot, "coverage_total"),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("baseline", help="path to current coverage baseline JSON")
    args = parser.parse_args()

    current_path = Path(args.baseline)
    current = read_json(current_path)

    base = git_show(args.baseline)
    if not base:
        base = load_fallback_baseline()

    current_covered = as_int(current, "coverage_covered")
    current_total = as_int(current, "coverage_total")
    base_covered = as_int(base, "coverage_covered")
    base_total = as_int(base, "coverage_total")

    if current_total <= 0:
        fail("invalid current coverage_total: must be > 0")
    if base_total <= 0:
        fail("invalid baseline coverage_total: must be > 0")

    current_pct = (current_covered / current_total) * 100.0
    base_pct = (base_covered / base_total) * 100.0
    coverage_delta = current_covered - base_covered
    percent_delta = current_pct - base_pct

    if current_covered < base_covered:
        fail(
            f"P15 coverage non-regression failed: covered dropped by {coverage_delta} "
            f"(current={current_covered}, baseline={base_covered})"
        )
    if current_pct + 1e-9 < base_pct:
        fail(
            "P15 coverage non-regression failed: percent dropped "
            f"from {base_pct:.2f}% to {current_pct:.2f}%"
        )

    print(
        "P15 coverage non-regression passed "
        f"(covered: {current_covered}/{current_total} = {current_pct:.2f}%, "
        f"baseline: {base_covered}/{base_total} = {base_pct:.2f}%, "
        f"delta={coverage_delta}, delta_pct={percent_delta:+.2f}%)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
