#!/usr/bin/env python3
"""P19 unified lint wrapper for compas.

Checks executed:
- Rust clippy (always)
- Ruff (if runtime Python sources are present)
- ESLint (if runtime JS/TS sources are present)
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from shutil import which
from typing import Iterable


ROOT = Path(__file__).resolve().parents[3]

IGNORED_DIRS = {
    ".git",
    ".next",
    ".venv",
    "build",
    "dist",
    "node_modules",
    "scripts",
    "target",
    "tools",
}

SKIP_RUNTIME_DIRS = {"scripts", "tools"}

ESLINT_CONFIGS = {
    ".eslintrc",
    ".eslintrc.cjs",
    ".eslintrc.js",
    ".eslintrc.json",
    ".eslintrc.yaml",
    ".eslintrc.yml",
    "eslint.config.cjs",
    "eslint.config.js",
    "eslint.config.mjs",
}


@dataclass
class ResultSummary:
    label: str
    exit_code: int
    command: list[str]
    stdout: str
    stderr: str
    skipped: bool = False


def _iter_source_files(suffixes: Iterable[str]) -> Iterable[Path]:
    suffixes_l = {s.lower() for s in suffixes}
    for current_root, dirs, files in os.walk(ROOT):
        dirs[:] = [d for d in dirs if d not in IGNORED_DIRS]
        for name in files:
            if Path(name).suffix.lower() in suffixes_l:
                yield Path(current_root) / name


def _has_sources(suffixes: Iterable[str], *, skip_runtime_artifacts: bool = False) -> bool:
    for path in _iter_source_files(suffixes):
        if skip_runtime_artifacts and any(part in SKIP_RUNTIME_DIRS for part in path.relative_to(ROOT).parts):
            continue
        return True
    return False


def _run(command: list[str], label: str) -> ResultSummary:
    proc = subprocess.run(
        command,
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        shell=False,
        check=False,
    )
    return ResultSummary(
        label=label,
        exit_code=proc.returncode,
        command=command,
        stdout=(proc.stdout or ""),
        stderr=(proc.stderr or ""),
    )


def _skip(label: str, reason: str, *, details: str = "") -> ResultSummary:
    note = f"[skip] {reason}"
    if details:
        note = f"{note}: {details}"
    return ResultSummary(label=label, exit_code=0, command=[], stdout=f"{note}\n", stderr="", skipped=True)


def _print_result(entry: ResultSummary) -> None:
    if entry.skipped:
        print(entry.stdout.rstrip("\n"))
        return
    print(f"=== {entry.label} :: {' '.join(entry.command)} ===")
    if entry.stdout.strip():
        print(entry.stdout.rstrip("\n"))
    if entry.stderr.strip():
        print(entry.stderr.rstrip("\n"))


def _format_command(tool: str, args: list[str]) -> list[str]:
    return [tool, *args]


def main() -> int:
    results: list[ResultSummary] = []
    any_failed = False

    clippy = _run(_format_command("cargo", ["clippy", "--workspace"]), "clippy")
    _print_result(clippy)
    if clippy.exit_code != 0:
        any_failed = True
    results.append(clippy)

    has_py = _has_sources({".py"}, skip_runtime_artifacts=True)
    has_js = _has_sources({".js", ".jsx", ".ts", ".tsx"}, skip_runtime_artifacts=True)

    if has_py:
        if which("ruff") is None:
            print("[warn] ruff requested but executable is not available; skipping")
            results.append(_skip("ruff", "missing executable"))
        else:
            ruff = _run(_format_command("ruff", ["check", "."]), "ruff")
            _print_result(ruff)
            if ruff.exit_code != 0:
                any_failed = True
            results.append(ruff)
    else:
        results.append(_skip("ruff", "no python sources in repo"))
        _print_result(results[-1])

    if has_js:
        if which("eslint") is None:
            print("[warn] eslint requested but executable is not available; skipping")
            results.append(_skip("eslint", "missing executable"))
        elif not any((ROOT / name).is_file() for name in ESLINT_CONFIGS):
            print("[warn] eslint requested but no config files were found; skipping")
            results.append(_skip("eslint", "missing eslint config"))
        else:
            eslint = _run(
                [
                    "eslint",
                    "--max-warnings",
                    "0",
                    "--ext",
                    ".js,.jsx,.ts,.tsx",
                    ".",
                ],
                "eslint",
            )
            _print_result(eslint)
            if eslint.exit_code != 0:
                any_failed = True
            results.append(eslint)
    else:
        results.append(_skip("eslint", "no js/ts sources in repo"))
        _print_result(results[-1])

    failures = [entry for entry in results if not entry.skipped and entry.exit_code != 0]
    skipped = [entry for entry in results if entry.skipped]

    print(
        f"P19 unified lint status: {'PASS' if not any_failed else 'FAIL'} "
        f"(checks={len(results)}, failed={len(failures)}, skipped={len(skipped)})"
    )
    return 1 if any_failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
