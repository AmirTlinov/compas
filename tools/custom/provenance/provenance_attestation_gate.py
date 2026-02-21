#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from shutil import which
from time import perf_counter
from typing import Any

from json import JSONDecodeError


FINDING_SEVERITY = ("low", "medium", "high", "critical")
REQUIRED_EVIDENCE_FILES = {"provenance-report.json"}


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def sha256_file(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as fp:
        for chunk in iter(lambda: fp.read(8192), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def git_head(repo_root: Path) -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=str(repo_root), text=True
        ).strip()
    except Exception:
        return ""


def add_finding(
    findings: list[dict[str, Any]],
    code: str,
    severity: str,
    category: str,
    message: str,
    path: str | None,
) -> None:
    findings.append(
        {
            "code": code,
            "severity": severity,
            "category": category,
            "message": message,
            "path": path,
            "line": None,
            "evidence_ref": "",
        }
    )


def validate_json_artifact(path: Path, findings: list[dict[str, Any]]) -> dict[str, Any] | None:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        add_finding(
            findings,
            code="P11.MISSING_FILE",
            severity="high",
            category="evidence.availability",
            message=f"Required evidence file is missing: {path}",
            path=str(path),
        )
        return None
    except JSONDecodeError:
        add_finding(
            findings,
            code="P11.INVALID_JSON",
            severity="high",
            category="evidence.schema",
            message=f"Evidence JSON is malformed: {path}",
            path=str(path),
        )
        return None

    if not isinstance(data, dict):
        add_finding(
            findings,
            code="P11.INVALID_SCHEMA",
            severity="medium",
            category="evidence.schema",
            message=f"Evidence JSON must be an object: {path}",
            path=str(path),
        )
        return None

    if not data.get("artifacts"):
        add_finding(
            findings,
            code="P11.EVIDENCE_EMPTY",
            severity="medium",
            category="evidence.schema",
            message=f"Evidence report has no artifacts list: {path}",
            path=str(path),
        )

    return data


def validate_report_artifact(report: Path, data: dict[str, Any], findings: list[dict[str, Any]]) -> None:
    if data.get("schema_version") not in {"provenance-attestation/v1", "p11-attestation-report/v1"}:
        add_finding(
            findings,
            code="P11.MISSING_SCHEMA_VERSION",
            severity="low",
            category="evidence.schema",
            message=f"Unexpected or missing provenance schema_version in {report}",
            path=str(report),
        )

    if data.get("status") not in {"pass", "warn", "ok", "fail", None}:
        add_finding(
            findings,
            code="P11.INVALID_STATUS",
            severity="medium",
            category="evidence.schema",
            message=f"Unexpected status value in {report}",
            path=str(report),
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--report-root", default="artifacts/provenance")
    parser.add_argument("--required-files", nargs="*", default=sorted(REQUIRED_EVIDENCE_FILES))
    parser.add_argument("--strict", action="store_true", help="Treat external tool misses as blocking")
    args = parser.parse_args()

    start = perf_counter()
    repo_root = Path(args.repo_root).resolve()
    report_root = repo_root / args.report_root

    findings: list[dict[str, Any]] = []

    if not report_root.exists() or not report_root.is_dir():
        add_finding(
            findings,
            code="P11.MISSING_EVIDENCE_ROOT",
            severity="high",
            category="evidence.availability",
            message=f"Provenance evidence directory does not exist: {report_root}",
            path=str(report_root),
        )
        report_data: list[dict[str, Any]] = []
    else:
        evidence_files = [
            p for p in report_root.iterdir() if p.is_file() and p.suffix.lower() in {".json", ".txt", ".sig", ".pem", ".crt"}
        ]
        if not evidence_files:
            add_finding(
                findings,
                code="P11.EMPTY_EVIDENCE_DIR",
                severity="high",
                category="evidence.availability",
                message=f"No provenance evidence files found in {report_root}",
                path=str(report_root),
            )

        report_data = []
        for name in args.required_files:
            path = report_root / name
            parsed = validate_json_artifact(path, findings)
            if parsed is not None:
                validate_report_artifact(path, parsed, findings)
                report_data.append(parsed)

    # Compatibility/compatibility checks are advisory to avoid blocking local development on optional binaries.
    for binary in ("cosign", "slsa-verifier", "cargo", "python3"):
        if which(binary) is None:
            findings.append(
                {
                    "code": "P11.MISSING_TOOL",
                    "severity": "low",
                    "category": "environment.tools",
                    "message": f"Optional tool '{binary}' is not available in PATH",
                    "path": binary,
                    "line": None,
                    "evidence_ref": "",
                }
            )
            if args.strict and binary in {"cosign", "slsa-verifier"}:
                add_finding(
                    findings,
                    code="P11.MISSING_MANDATORY_TOOL",
                    severity="high",
                    category="environment.tools",
                    message=(
                        f"Mandatory provenance tool '{binary}' is not available and --strict was requested"
                    ),
                    path=binary,
                )

    for finding in findings:
        sev = finding["severity"]
        if sev not in FINDING_SEVERITY:
            finding["severity"] = "medium"

    blocking = [f for f in findings if f["severity"] in {"high", "critical"}]

    evidence_payload = {
        "stdout_hash": "",
        "stderr_hash": "",
        "report_hash": None,
        "report_path": str(report_root / "provenance-report.json"),
        "commit_sha": git_head(repo_root),
    }

    report_path = report_root / "provenance-report.json"
    if report_path.exists() and report_path.is_file():
        evidence_payload["report_hash"] = sha256_file(report_path)

    result = {
        "adapter_result": {
            "status": "fail" if blocking else "pass",
            "plugin_id": "P11",
            "adapter_id": "python3-attestation-adapter",
            "tool": {
                "command": "python3",
                "version": "1.0",
            },
            "findings": findings,
            "metrics": {
                "duration_ms": int((perf_counter() - start) * 1000),
                "findings_total": len(findings),
                "warnings_total": len([f for f in findings if f["severity"] == "low"]),
            },
            "evidence": evidence_payload,
        },
        "report_data": report_data,
    }

    payload = json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2)
    result["adapter_result"]["evidence"]["stdout_hash"] = sha256_text(payload)
    payload = json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2)

    print(payload)
    return 1 if blocking else 0


if __name__ == "__main__":
    sys.exit(main())
