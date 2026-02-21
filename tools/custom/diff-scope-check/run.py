#!/usr/bin/env python3
from __future__ import annotations

import fnmatch
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List

try:
    import tomllib  # py311+
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib  # type: ignore


PLUGIN_ID = "p03"
ADAPTER_ID = "python-diff-scope"
SCOPE_FILE = Path(".agents/mcp/compas/plugins/p03/scope.toml")


class AdapterError(RuntimeError):
    """Adapter/runtime failure that must be treated as fail-closed."""


def sha256_hex(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def run_git(args: List[str], repo_root: Path) -> str:
    proc = subprocess.run(
        ["git", *args],
        cwd=str(repo_root),
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        stderr = proc.stderr.strip() or proc.stdout.strip()
        raise AdapterError(f"git {" ".join(args)} failed: {stderr}")
    return proc.stdout.strip()


def resolve_diff_base(repo_root: Path) -> str:
    env_base = os.environ.get("P03_DIFF_BASE", "").strip()
    if env_base:
        # Explicit override used by local debugging / tests.
        return env_base

    for candidate in ["origin/main", "origin/master", "HEAD~1", "HEAD"]:
        try:
            run_git(["rev-parse", "--verify", candidate], repo_root)
            return candidate
        except AdapterError:
            continue

    raise AdapterError(
        "unable to resolve git diff base. checked: origin/main, origin/master, HEAD~1, HEAD"
    )


def list_changed_files(repo_root: Path, base: str) -> List[str]:
    raw = run_git(["diff", "--name-only", f"{base}...HEAD"], repo_root)
    return [
        line.strip().replace("\\", "/")
        for line in raw.splitlines()
        if line.strip()
    ]


def load_scope(repo_root: Path) -> Dict[str, Any]:
    scope_path = repo_root / SCOPE_FILE
    if not scope_path.is_file():
        raise AdapterError(f"missing scope contract: {scope_path}")

    with scope_path.open("rb") as f:
        payload = tomllib.load(f)

    scope = payload.get("scope")
    if not isinstance(scope, dict):
        raise AdapterError("scope contract must define [scope] section")

    allowed = scope.get("allowed_globs", [])
    if not isinstance(allowed, list) or not allowed:
        raise AdapterError("scope.allowed_globs must be a non-empty array")

    allowed_globs: List[str] = []
    for item in allowed:
        if not isinstance(item, str) or not item.strip():
            raise AdapterError("scope.allowed_globs must contain non-empty strings")
        allowed_globs.append(item.strip())

    return {
        "owner": str(scope.get("owner", "unknown")),
        "version": str(scope.get("version", "1.0.0")),
        "description": str(scope.get("description", "")),
        "allowed_globs": allowed_globs,
    }


def in_scope(path: str, patterns: List[str]) -> bool:
    return any(fnmatch.fnmatch(path, pattern) for pattern in patterns)


def build_findings(unmatched: List[str]) -> List[Dict[str, Any]]:
    if not unmatched:
        return []

    findings: List[Dict[str, Any]] = [
        {
            "code": "P03-SCOPE-MISMATCH",
            "severity": "critical",
            "category": "plan_diff_scope",
            "message": "changed files are outside declared P03 scope",
            "path": None,
            "line": None,
            "evidence_ref": "diff-scope-check",
        }
    ]

    for path in unmatched:
        findings.append(
            {
                "code": "P03-SCOPE-FILE",
                "severity": "high",
                "category": "plan_diff_scope",
                "message": "file is not allowed by scope contract",
                "path": path,
                "line": None,
                "evidence_ref": "diff-scope-check",
            }
        )

    return findings


def build_result(
    status: str,
    findings: List[Dict[str, Any]],
    changed_files: List[str],
    matched_files: List[str],
    scope_cfg: Dict[str, Any],
    stderr_text: str,
    repo_root: Path,
) -> str:
    command = "python3"
    commit_sha = "unknown"
    try:
        commit_sha = run_git(["rev-parse", "HEAD"], repo_root)
    except AdapterError:
        pass

    duration_ms = int(time.time() * 1000) - start_ms
    payload = {
        "adapter_result": {
            "status": status,
            "plugin_id": PLUGIN_ID,
            "adapter_id": ADAPTER_ID,
            "tool": {
                "command": command,
                "version": f"Python {sys.version.split()[0]}",
            },
            "findings": findings,
            "metrics": {
                "duration_ms": duration_ms,
                "findings_total": len(findings),
                "warnings_total": len([f for f in findings if f.get("severity") == "low"]),
            },
            "evidence": {
                "stdout_hash": "",
                "stderr_hash": sha256_hex(stderr_text.encode()),
                "report_hash": None,
                "report_path": None,
                "commit_sha": commit_sha,
            },
            "scope": {
                "version": scope_cfg.get("version", "1.0.0"),
                "owner": scope_cfg.get("owner", "unknown"),
                "description": scope_cfg.get("description", ""),
                "total_changed": len(changed_files),
                "matched": len(matched_files),
                "unmatched": [p for p in changed_files if p not in matched_files],
            },
        }
    }

    text = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True)
    payload["adapter_result"]["evidence"]["stdout_hash"] = sha256_hex(text.encode())

    text = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True)
    payload["adapter_result"]["evidence"]["report_hash"] = sha256_hex(text.encode())
    return json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True)


def run() -> int:
    global start_ms
    start_ms = int(time.time() * 1000)
    repo_root = Path.cwd().resolve()

    stderr_parts: List[str] = []
    try:
        scope = load_scope(repo_root)
        base = resolve_diff_base(repo_root)
        changed = list_changed_files(repo_root, base)
        unmatched: List[str] = [p for p in changed if not in_scope(p, scope["allowed_globs"])]
        findings = build_findings(unmatched)
        matched = [p for p in changed if p not in unmatched]

        status = "pass" if not findings else "fail"
        if not findings:
            stderr_parts.append(
                f"scope contract matched {len(changed)} changed paths for plugin {PLUGIN_ID}"
            )

        output = build_result(
            status=status,
            findings=findings,
            changed_files=changed,
            matched_files=matched,
            scope_cfg=scope,
            stderr_text="\n".join(stderr_parts),
            repo_root=repo_root,
        )
        print(output)
        return 0 if status == "pass" else 1

    except AdapterError as exc:
        status = "error"
        stderr_text = f"P03 adapter failure: {exc}"
        output = build_result(
            status=status,
            findings=[
                {
                    "code": "P03-ADAPTER-ERROR",
                    "severity": "critical",
                    "category": "runtime",
                    "message": str(exc),
                    "path": None,
                    "line": None,
                    "evidence_ref": "diff-scope-check",
                }
            ],
            changed_files=[],
            matched_files=[],
            scope_cfg={"version": "1.0.0", "owner": "unknown", "description": ""},
            stderr_text=stderr_text,
            repo_root=repo_root,
        )
        print(output)
        print(stderr_text, file=sys.stderr)
        return 2
    except Exception as exc:  # pragma: no cover
        status = "error"
        stderr_text = f"P03 unexpected adapter exception: {exc}"
        output = build_result(
            status=status,
            findings=[
                {
                    "code": "P03-ADAPTER-EXCEPTION",
                    "severity": "critical",
                    "category": "runtime",
                    "message": str(exc),
                    "path": None,
                    "line": None,
                    "evidence_ref": "diff-scope-check",
                }
            ],
            changed_files=[],
            matched_files=[],
            scope_cfg={"version": "1.0.0", "owner": "unknown", "description": ""},
            stderr_text=stderr_text,
            repo_root=repo_root,
        )
        print(output)
        print(stderr_text, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(run())
