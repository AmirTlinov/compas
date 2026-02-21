#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


PLUGIN_ID = "p13"
ADAPTER_ID = "secrets-scan-adapter"
SEVERITY_DEFAULT = "medium"


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def git_commit_sha(repo_root: Path) -> str:
    try:
        completed = subprocess.run(
            ["git", "rev-parse", "--verify", "HEAD"],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=True,
        )
        return completed.stdout.strip() or "<unknown>"
    except Exception:
        return "<unknown>"


@dataclass
class Finding:
    code: str
    severity: str
    category: str
    message: str
    path: str | None
    line: int | None
    evidence_ref: str

    def to_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "severity": self.severity,
            "category": self.category,
            "message": self.message,
            "path": self.path,
            "line": self.line,
            "evidence_ref": self.evidence_ref,
        }


@dataclass
class ScannerResult:
    scanner: str
    ok: bool
    status: str
    findings: list[Finding]
    duration_ms: int
    warnings: int = 0
    exit_code: int | None = None
    raw_output: str = ""
    command: list[str] = None
    version: str = "unknown"

    def __post_init__(self) -> None:
        if self.command is None:
            self.command = []


def _norm_code(code: str, severity: str, fallback: str) -> str:
    return str(code).strip() or fallback


def _map_severity(source: str, raw: str) -> str:
    raw_norm = (raw or "").strip().lower()
    if source == "semgrep":
        table = {
            "critical": "critical",
            "error": "critical",
            "high": "high",
            "warning": "medium",
            "medium": "medium",
            "info": "low",
            "low": "low",
        }
    else:
        table = {
            "critical": "critical",
            "high": "high",
            "medium": "medium",
            "low": "low",
            "warning": "medium",
            "info": "low",
        }

    if raw_norm in table:
        return table[raw_norm]
    raise ValueError(f"unknown severity '{raw}' for {source}")


def _add_unknown_severity(findings: list[Finding], source: str, item: str) -> None:
    findings.append(
        Finding(
            code="p13.severity.unknown",
            severity="critical",
            category="adapter",
            message=f"{source}: failed to map severity for item: {item}",
            path=None,
            line=None,
            evidence_ref="p13.adapter",
        )
    )


def _run_command(
    cmd: list[str],
    cwd: Path,
    timeout_seconds: int,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
        timeout=timeout_seconds,
        check=False,
    )


def _version(cmd: list[str]) -> str:
    try:
        completed = _run_command([cmd[0], "--version"], Path("."), 10)
        return (completed.stdout or completed.stderr or "").splitlines()[0].strip() or "unknown"
    except Exception:
        return "unknown"


def _scan_semgrep(repo_root: Path) -> ScannerResult:
    name = "semgrep"
    if shutil.which("semgrep") is None:
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=[
                Finding(
                    code="p13.tool.missing",
                    severity="critical",
                    category="infrastructure",
                    message="semgrep is not installed or not found in PATH",
                    path=".github", 
                    line=None,
                    evidence_ref="p13.adapter",
                )
            ],
            duration_ms=0,
            exit_code=127,
            command=[name],
            version="missing",
        )

    cmd = [
        "semgrep",
        "scan",
        "--config",
        "auto",
        "--json",
        "--quiet",
        str(repo_root),
    ]
    start = time.time()
    completed = _run_command(cmd, repo_root, timeout_seconds=120)
    duration_ms = int((time.time() - start) * 1000)
    findings: list[Finding] = []

    if completed.returncode not in {0, 1}:  # semgrep typically returns 1 on findings
        msg = (completed.stderr or completed.stdout or "semgrep failed").strip()
        findings.append(
            Finding(
                code="p13.semgrep.command_failed",
                severity="critical",
                category="infrastructure",
                message=f"semgrep execution failed: {msg}",
                path=None,
                line=None,
                evidence_ref="p13.adapter.semgrep",
            )
        )
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=findings,
            duration_ms=duration_ms,
            exit_code=completed.returncode,
            raw_output=completed.stdout[:4000],
            command=cmd,
            version=_version(["semgrep"]),
        )

    try:
        payload = json.loads(completed.stdout or "{}")
    except Exception as exc:
        findings.append(
            Finding(
                code="p13.semgrep.invalid_output",
                severity="critical",
                category="adapter",
                message=f"Failed to parse semgrep JSON: {exc}",
                path=None,
                line=None,
                evidence_ref="p13.adapter.semgrep",
            )
        )
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=findings,
            duration_ms=duration_ms,
            exit_code=completed.returncode,
            raw_output=completed.stdout[:4000],
            command=cmd,
            version=_version(["semgrep"]),
        )

    for item in payload.get("results", []) if isinstance(payload, dict) else []:
        try:
            raw_sev = (item.get("extra", {}) or {}).get("severity", "medium")
            severity = _map_severity("semgrep", raw_sev)
        except ValueError:
            _add_unknown_severity(findings, "semgrep", str(item.get("check_id", "unknown")))
            continue

        path = item.get("path")
        line: int | None = None
        start_meta = item.get("start")
        if isinstance(start_meta, dict):
            raw_line = start_meta.get("line")
            if isinstance(raw_line, int):
                line = raw_line
        findings.append(
            Finding(
                code=_norm_code(item.get("check_id"), "p13.semgrep.rule", "p13.semgrep.match"),
                severity=severity,
                category="secrets",
                message=str(item.get("extra", {}).get("message", "semgrep finding")).strip()
                or "semgrep finding",
                path=path,
                line=line,
                evidence_ref="p13.adapter.semgrep",
            )
        )

    return ScannerResult(
        scanner=name,
        ok=len([f for f in findings if f.severity in {"critical", "high", "medium", "low"}]) == 0,
        status="pass" if not findings else "fail",
        findings=findings,
        duration_ms=duration_ms,
        exit_code=completed.returncode,
        raw_output=completed.stdout[:4000],
        command=cmd,
        version=_version(["semgrep"]),
    )


def _extract_gitleaks_finding(item: dict[str, Any], source: str) -> Finding | None:
    try:
        raw_sev = str(item.get("Severity") or item.get("severity") or "medium")
        severity = _map_severity("gitleaks", raw_sev)
    except ValueError:
        _add_unknown_severity([], source, str(item.get("Description") or item.get("RuleID") or "unknown"))
        return None

    path = item.get("File") or item.get("file") or item.get("Path")
    line = item.get("StartLine") or item.get("start_line")
    message = (
        item.get("Description")
        or item.get("Match")
        or item.get("RuleID")
        or item.get("RuleId")
        or "gitleaks finding"
    )
    code = item.get("RuleID") or item.get("RuleId") or "gitleaks.match"
    if isinstance(line, float) and line.is_integer():
        line = int(line)
    elif isinstance(line, str) and line.isdigit():
        line = int(line)

    return Finding(
        code=str(code),
        severity=severity,
        category="secrets",
        message=str(message).strip(),
        path=str(path) if path is not None else None,
        line=int(line) if isinstance(line, int) and line > 0 else None,
        evidence_ref="p13.adapter.gitleaks",
    )


def _scan_gitleaks(repo_root: Path) -> ScannerResult:
    name = "gitleaks"
    if shutil.which("gitleaks") is None:
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=[
                Finding(
                    code="p13.tool.missing",
                    severity="critical",
                    category="infrastructure",
                    message="gitleaks is not installed or not found in PATH",
                    path=None,
                    line=None,
                    evidence_ref="p13.adapter",
                )
            ],
            duration_ms=0,
            exit_code=127,
            command=[name],
            version="missing",
        )

    cmd = [
        "gitleaks",
        "detect",
        "--no-git",
        "--json",
        str(repo_root),
    ]
    start = time.time()
    completed = _run_command(cmd, repo_root, timeout_seconds=120)
    duration_ms = int((time.time() - start) * 1000)
    findings: list[Finding] = []

    out = (completed.stdout or "") + (completed.stderr or "")
    parsed_items: list[dict[str, Any]] = []

    if completed.returncode not in {0, 1}:
        msg = (completed.stderr or completed.stdout or "gitleaks failed").strip()
        findings.append(
            Finding(
                code="p13.gitleaks.command_failed",
                severity="critical",
                category="infrastructure",
                message=f"gitleaks execution failed: {msg}",
                path=None,
                line=None,
                evidence_ref="p13.adapter.gitleaks",
            )
        )
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=findings,
            duration_ms=duration_ms,
            exit_code=completed.returncode,
            raw_output=out[:4000],
            command=cmd,
            version=_version(["gitleaks"]),
        )

    for line in out.splitlines():
        l = line.strip()
        if not l:
            continue
        try:
            payload = json.loads(l)
        except Exception:
            continue

        if isinstance(payload, dict) and payload.get("Type") == "json":
            continue
        if isinstance(payload, dict):
            parsed_items.append(payload)
        elif isinstance(payload, list):
            parsed_items.extend([item for item in payload if isinstance(item, dict)])

    if not parsed_items and out.strip() and not out.strip().startswith("["):
        # Keep this path as adapter error only if command output is non-empty but unparseable.
        try:
            json.loads(out)
        except Exception:
            # Non-JSON output from gitleaks means the command format mismatch.
            findings.append(
                Finding(
                    code="p13.gitleaks.invalid_output",
                    severity="critical",
                    category="adapter",
                    message="gitleaks output is not JSON and not parseable as findings",
                    path=None,
                    line=None,
                    evidence_ref="p13.adapter.gitleaks",
                )
            )
            return ScannerResult(
                scanner=name,
                ok=False,
                status="error",
                findings=findings,
                duration_ms=duration_ms,
                exit_code=completed.returncode,
                raw_output=out[:4000],
                command=cmd,
                version=_version(["gitleaks"]),
            )

    for item in parsed_items:
        if not isinstance(item, dict):
            continue
        parsed = _extract_gitleaks_finding(item, "gitleaks")
        if parsed is not None:
            findings.append(parsed)

    status = "fail" if findings else "pass"
    return ScannerResult(
        scanner=name,
        ok=completed.returncode == 0 and not findings,
        status=status,
        findings=findings,
        duration_ms=duration_ms,
        exit_code=completed.returncode,
        raw_output=out[:4000],
        command=cmd,
        version=_version(["gitleaks"]),
    )


def _extract_trufflehog_finding(item: dict[str, Any]) -> Finding:
    path = None
    line = None
    message = item.get("DetectorName") or item.get("detector_name") or item.get("rule") or "trufflehog finding"
    code = item.get("DetectorName") or item.get("detector_name") or "trufflehog.match"

    source = item.get("SourceMetadata") or {}
    file_path = None
    if isinstance(source, dict):
        data = source.get("Data") or source.get("data") or {}
        if isinstance(data, dict):
            file_path = data.get("Path") or data.get("path")
            line = data.get("Line") or data.get("line")
        if file_path is None:
            file_path = source.get("file") or source.get("File")

    if file_path is None:
        file_path = item.get("File") or item.get("file")

    if isinstance(line, str) and line.isdigit():
        line = int(line)
    elif isinstance(line, float) and line.is_integer():
        line = int(line)

    return Finding(
        code=str(code),
        severity="critical",
        category="secrets",
        message=str(message),
        path=str(file_path) if file_path else None,
        line=int(line) if isinstance(line, int) and line > 0 else None,
        evidence_ref="p13.adapter.trufflehog",
    )


def _scan_trufflehog(repo_root: Path) -> ScannerResult:
    name = "trufflehog"
    if shutil.which("trufflehog") is None:
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=[
                Finding(
                    code="p13.tool.missing",
                    severity="critical",
                    category="infrastructure",
                    message="trufflehog is not installed or not found in PATH",
                    path=None,
                    line=None,
                    evidence_ref="p13.adapter",
                )
            ],
            duration_ms=0,
            exit_code=127,
            command=[name],
            version="missing",
        )

    cmd = [
        "trufflehog",
        "filesystem",
        str(repo_root),
        "--json",
    ]
    start = time.time()
    completed = _run_command(cmd, repo_root, timeout_seconds=180)
    duration_ms = int((time.time() - start) * 1000)
    findings: list[Finding] = []

    if completed.returncode not in {0, 1}:
        msg = (completed.stderr or completed.stdout or "trufflehog failed").strip()
        findings.append(
            Finding(
                code="p13.trufflehog.command_failed",
                severity="critical",
                category="infrastructure",
                message=f"trufflehog execution failed: {msg}",
                path=None,
                line=None,
                evidence_ref="p13.adapter.trufflehog",
            )
        )
        return ScannerResult(
            scanner=name,
            ok=False,
            status="error",
            findings=findings,
            duration_ms=duration_ms,
            exit_code=completed.returncode,
            raw_output=(completed.stdout or "")[:4000],
            command=cmd,
            version=_version(["trufflehog"]),
        )

    out = completed.stdout.strip().splitlines()
    for raw in out:
        raw = raw.strip()
        if not raw:
            continue
        try:
            data = json.loads(raw)
        except Exception:
            continue

        if not isinstance(data, dict):
            continue

        findings.append(_extract_trufflehog_finding(data))

    status = "fail" if findings else "pass"
    return ScannerResult(
        scanner=name,
        ok=completed.returncode == 0 and not findings,
        status=status,
        findings=findings,
        duration_ms=duration_ms,
        exit_code=completed.returncode,
        raw_output=(completed.stdout or "")[:4000],
        command=cmd,
        version=_version(["trufflehog"]),
    )


def main() -> int:
    repo_root = Path(os.environ.get("AI_DX_REPO_ROOT") or Path.cwd()).resolve()
    start_ts = time.time()

    scanners = [_scan_semgrep, _scan_gitleaks, _scan_trufflehog]
    all_findings: list[Finding] = []
    scanner_states: list[dict[str, Any]] = []

    overall_status = "pass"

    for scan in scanners:
        state = scan(repo_root)
        scanner_states.append(
            {
                "name": state.scanner,
                "command": state.command,
                "exit_code": state.exit_code,
                "duration_ms": state.duration_ms,
                "version": state.version,
                "ok": state.ok,
                "status": state.status,
            }
        )
        all_findings.extend(state.findings)

        if state.status == "error":
            overall_status = "error"
        elif state.status == "fail" and overall_status != "error":
            overall_status = "fail"

    total_seconds = int((time.time() - start_ts) * 1000)
    warnings_total = sum(1 for f in all_findings if f.severity == "low")

    payload = {
        "adapter_result": {
            "status": overall_status,
            "plugin_id": PLUGIN_ID,
            "adapter_id": ADAPTER_ID,
            "scanners": scanner_states,
            "findings": [f.to_dict() for f in all_findings],
            "metrics": {
                "duration_ms": total_seconds,
                "findings_total": len(all_findings),
                "warnings_total": warnings_total,
            },
            "evidence": {
                "stdout_hash": "",
                "stderr_hash": "",
                "report_hash": "",
                "report_path": None,
                "commit_sha": git_commit_sha(repo_root),
            },
        }
    }

    report_text = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    payload["adapter_result"]["evidence"]["stdout_hash"] = sha256_text(report_text)
    payload["adapter_result"]["evidence"]["report_hash"] = sha256_text(report_text)

    report_line = json.dumps(payload, ensure_ascii=False)
    print("P13-SECRETS-SCAN status=%s findings=%d" % (overall_status, len(all_findings)))
    print(report_line)

    if overall_status == "pass":
        return 0
    if overall_status == "fail":
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
