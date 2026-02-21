#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class CheckResult:
    code: str
    severity: str
    category: str
    message: str
    path: str | None
    line: int | None
    evidence_ref: str


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()[:16]


def run_cmd(command: list[str], cwd: Path, timeout_ms: int = 120000) -> dict[str, Any]:
    started = time.perf_counter()
    try:
        proc = subprocess.run(
            command,
            cwd=str(cwd),
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_ms / 1000,
        )
    except subprocess.TimeoutExpired as exc:
        return {
            "command": command,
            "success": False,
            "exit_code": -1,
            "timed_out": True,
            "duration_ms": int((time.perf_counter() - started) * 1000),
            "stdout": exc.stdout or "",
            "stderr": exc.stderr or "",
        }
    except Exception as exc:  # pragma: no cover - hard fail path
        return {
            "command": command,
            "success": False,
            "exit_code": -2,
            "timed_out": False,
            "duration_ms": int((time.perf_counter() - started) * 1000),
            "stdout": "",
            "stderr": f"command invocation failed: {exc}",
            "error": str(exc),
        }

    return {
        "command": command,
        "success": proc.returncode == 0,
        "exit_code": proc.returncode,
        "timed_out": False,
        "duration_ms": int((time.perf_counter() - started) * 1000),
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def add_finding(findings: list[CheckResult], item: CheckResult) -> None:
    findings.append(item)


def severity_from_exit_code(rc: int) -> str:
    if rc == 0:
        return "low"
    if rc == 101:
        return "medium"
    return "high"


def check_docs_sync(repo_root: Path, findings: list[CheckResult]) -> dict[str, Any]:
    result = run_cmd(["python3", "scripts/docs_sync.py", "--check"], repo_root)
    if not result["success"]:
        add_finding(
            findings,
            CheckResult(
                code="P17.DS.DRIFT",
                severity=severity_from_exit_code(result["exit_code"]),
                category="docs_sync",
                message="docs_sync.py --check failed: architecture/docs auto-block is out of sync",
                path="scripts/docs_sync.py",
                line=None,
                evidence_ref="stdout",
            ),
        )
    return result


def check_optional_builder(
    repo_root: Path,
    config_name: str,
    command: str,
    args: list[str],
    findings: list[CheckResult],
    *,
    fail_on_missing: bool,
) -> dict[str, Any] | None:
    cfg = repo_root / config_name
    if not cfg.exists():
        return None

    if shutil.which(command) is None:
        severity = "high" if fail_on_missing else "low"
        add_finding(
            findings,
            CheckResult(
                code=f"P17.DOCS.GENERATOR_MISSING_{command.upper()}",
                severity=severity,
                category="docs_generator",
                message=(
                    f"configured doc generator '{command}' is missing but '{config_name}' is present"
                ),
                path=str(cfg.relative_to(repo_root)),
                line=None,
                evidence_ref="command",
            ),
        )
        return None

    result = run_cmd([command, *args], repo_root)
    if not result["success"]:
        add_finding(
            findings,
            CheckResult(
                code=f"P17.DOCS.GENERATOR_FAIL_{command.upper()}",
                severity=severity_from_exit_code(result["exit_code"]),
                category="docs_generator",
                message=f"failed to run '{command}': command exit {result['exit_code']}",
                path=str(cfg.relative_to(repo_root)),
                line=None,
                evidence_ref="stdout",
            ),
        )

    return result


def run(repo_root: Path, json_output: bool) -> int:
    start_ms = int(time.time() * 1000)
    findings: list[CheckResult] = []

    cmd_results: dict[str, dict[str, Any]] = {}
    cmd_results["docs_sync"] = check_docs_sync(repo_root, findings)

    # Optional checks for repo-local generators:
    # keep these checks strict only when configuration is present.
    cmd_results["mkdocs"] = check_optional_builder(
        repo_root,
        "mkdocs.yml",
        "mkdocs",
        ["--version"],
        findings,
        fail_on_missing=True,
    )
    cmd_results["mdbook"] = check_optional_builder(
        repo_root,
        "book.toml",
        "mdbook",
        ["--version"],
        findings,
        fail_on_missing=True,
    )

    commit_sha = "unknown"
    git = run_cmd(["git", "rev-parse", "HEAD"], repo_root)
    if git["success"] and git["stdout"].strip():
        commit_sha = git["stdout"].strip()

    blocking = [f for f in findings if f.severity in {"high", "medium", "critical"}]
    status = "pass" if not blocking else "fail"

    result = {
        "adapter_result": {
            "status": status,
            "plugin_id": "p17",
            "adapter_id": "p17_docs_no_drift",
            "tool": {
                "command": "python3",
                "version": "1.0.0",
            },
            "findings": [
                {
                    "code": item.code,
                    "severity": item.severity,
                    "category": item.category,
                    "message": item.message,
                    "path": item.path,
                    "line": item.line,
                    "evidence_ref": item.evidence_ref,
                }
                for item in findings
            ],
            "metrics": {
                "duration_ms": int(time.time() * 1000) - start_ms,
                "findings_total": len(findings),
                "warnings_total": sum(
                    1 for item in findings if item.severity == "low"
                ),
            },
            "evidence": {
                "stdout_hash": sha256_text(json.dumps(cmd_results, sort_keys=True)),
                "stderr_hash": "n/a",
                "report_hash": None,
                "report_path": None,
                "commit_sha": commit_sha,
            },
        }
    }

    # compute report hash after structure assembled
    result["adapter_result"]["evidence"]["report_hash"] = hashlib.sha256(
        json.dumps(result, ensure_ascii=False, sort_keys=True).encode("utf-8")
    ).hexdigest()[:16]
    result_json = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True)

    if json_output:
        sys.stdout.write(result_json)
        if not result_json.endswith("\n"):
            sys.stdout.write("\n")

    if status == "fail":
        return 1
    if status == "error":
        return 2
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate docs sync/no-drift rules for plugin P17")
    parser.add_argument("--repo-root", default=str(Path(__file__).resolve().parent.parent))
    parser.add_argument("--json", action="store_true", default=True)
    args = parser.parse_args()

    repo_root = Path(args.repo_root).resolve()
    if not repo_root.exists():
        sys.stderr.write(f"repo root does not exist: {repo_root}\n")
        return 2

    return run(repo_root, json_output=args.json)


if __name__ == "__main__":
    raise SystemExit(main())
