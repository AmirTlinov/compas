#!/usr/bin/env python3
"""P21 CI/CD release readiness checker.

Fail-closed checks:
- repository invariants in workspace manifests and release assets
- mandatory presence of P21 plugin and release-check tool manifests

Output is intentionally stable and includes structured evidence in JSON.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SEVERITY_ORDER = {"low": 1, "medium": 2, "high": 3, "critical": 4}
FAIL_SEVERITIES = {"high", "critical"}


@dataclass(frozen=True)
class Finding:
    code: str
    severity: str
    category: str
    message: str
    path: str | None
    line: int | None
    evidence_ref: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="P21 release readiness checks")
    parser.add_argument(
        "--repo-root",
        required=True,
        help="Absolute or relative repository root",
    )
    return parser.parse_args()


def read_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as fh:
            return tomllib.load(fh)
    except (FileNotFoundError, tomllib.TOMLDecodeError) as exc:
        raise RuntimeError(f"failed to read toml {path}: {exc}") from exc


def hash_text(data: str) -> str:
    return hashlib.sha256(data.encode("utf-8")).hexdigest()[:16]


def check_workspace_root(repo_root: Path, findings: list[Finding], evidence: dict[str, str]) -> None:
    path = repo_root / "Cargo.toml"
    manifest = read_toml(path)
    if "workspace" not in manifest:
        findings.append(
            Finding(
                code="p21.workspace.missing",
                severity="critical",
                category="release.workspace",
                message="workspace manifest missing [workspace] section",
                path=str(path),
                line=None,
                evidence_ref="cargo.workspace.root",
            )
        )
        return

    members = manifest.get("workspace", {}).get("members", [])
    if not isinstance(members, list) or not members:
        findings.append(
            Finding(
                code="p21.workspace.members.invalid",
                severity="high",
                category="release.workspace",
                message="workspace manifest must list at least one member",
                path=str(path),
                line=None,
                evidence_ref="cargo.workspace.members",
            )
        )
        members = []

    evidence["workspace_members"] = ",".join(members)
    evidence["cargo_workspace_present"] = "true"

    semver_re = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")

    for member in members:
        if not isinstance(member, str):
            findings.append(
                Finding(
                    code="p21.workspace.members.type",
                    severity="high",
                    category="release.workspace",
                    message="workspace member entry must be string",
                    path=str(path),
                    line=None,
                    evidence_ref="cargo.workspace.members",
                )
            )
            continue
        member_manifest = repo_root / member / "Cargo.toml"
        if not member_manifest.exists():
            findings.append(
                Finding(
                    code="p21.workspace.member_missing",
                    severity="high",
                    category="release.workspace",
                    message=f"workspace member manifest not found: {member}",
                    path=str(member_manifest),
                    line=None,
                    evidence_ref="cargo.workspace.members",
                )
            )
            continue

        member_data = read_toml(member_manifest)
        package = member_data.get("package")
        if not isinstance(package, dict):
            findings.append(
                Finding(
                    code="p21.package.missing",
                    severity="high",
                    category="release.package",
                    message=f"{member_manifest} must contain [package]",
                    path=str(member_manifest),
                    line=None,
                    evidence_ref="cargo.package.table",
                )
            )
            continue

        version = package.get("version", "")
        if not isinstance(version, str) or not semver_re.match(version):
            findings.append(
                Finding(
                    code="p21.package.version_invalid",
                    severity="high",
                    category="release.version",
                    message=f"package version must be semver, got {version!r}",
                    path=str(member_manifest),
                    line=None,
                    evidence_ref="cargo.package.version",
                )
            )


def check_required_readiness_assets(repo_root: Path, findings: list[Finding], evidence: dict[str, str]) -> None:
    required = {
        "README.md": "release.documentation.readme",
        "Cargo.lock": "release.artifacts.lockfile",
        "CHANGELOG.md": "release.documentation.changelog",
        ".agents/mcp/compas/plugins/p21/plugin.toml": "release.plugin.config",
        "tools/custom/release-check/tool.toml": "release.plugin.tool_manifest",
    }

    missing = []
    for path_str, code in required.items():
        path = repo_root / path_str
        if not path.exists():
            missing.append(path_str)
            findings.append(
                Finding(
                    code=code,
                    severity="low",
                    category="release.assets",
                    message=f"Required readiness asset missing: {path_str}",
                    path=str(path),
                    line=None,
                    evidence_ref="release.assets.presence",
                )
            )

    if missing:
        evidence["missing_readiness_assets"] = ",".join(missing)
    else:
        evidence["missing_readiness_assets"] = "none"

    if (repo_root / "CHANGELOG.md").exists():
        evidence["changelog_present"] = "true"
    else:
        evidence["changelog_present"] = "false"


def check_p21_manifests(repo_root: Path, findings: list[Finding], evidence: dict[str, str]) -> None:
    plugin_path = repo_root / ".agents/mcp/compas/plugins/p21/plugin.toml"
    if not plugin_path.exists():
        # Already reported above, no duplicate.
        return

    plugin_cfg = read_toml(plugin_path)
    plugin = plugin_cfg.get("plugin", {})
    if plugin.get("id") != "p21":
        findings.append(
            Finding(
                code="p21.plugin.id_mismatch",
                severity="medium",
                category="release.plugin",
                message="plugin.toml must have id = \"p21\"",
                path=str(plugin_path),
                line=None,
                evidence_ref="p21.plugin.id",
            )
        )
    description = plugin.get("description", "").strip()
    evidence["plugin_id"] = str(plugin.get("id", ""))
    evidence["plugin_description_hash"] = hash_text(description)

    tool_path = repo_root / "tools/custom/release-check/tool.toml"
    if not tool_path.exists():
        return
    tool_cfg = read_toml(tool_path)
    tool = tool_cfg.get("tool", {})
    if tool.get("id") != "release-check":
        findings.append(
            Finding(
                code="p21.tool.id_mismatch",
                severity="medium",
                category="release.tool",
                message="tool.toml must define id = \"release-check\"",
                path=str(tool_path),
                line=None,
                evidence_ref="p21.tool.id",
            )
        )
    evidence["tool_command"] = str(tool.get("command", ""))

    if tool.get("command") != "python3":
        findings.append(
            Finding(
                code="p21.tool.command",
                severity="low",
                category="release.tool",
                message="release-check tool should use python3",
                path=str(tool_path),
                line=None,
                evidence_ref="p21.tool.command",
            )
        )


def run_checks(repo_root: Path) -> tuple[str, list[Finding], dict[str, str]]:
    findings: list[Finding] = []
    evidence: dict[str, str] = {}

    check_workspace_root(repo_root, findings, evidence)
    check_required_readiness_assets(repo_root, findings, evidence)
    check_p21_manifests(repo_root, findings, evidence)

    findings = sorted(
        findings,
        key=lambda item: (SEVERITY_ORDER.get(item.severity, 999), item.code),
    )

    status = "pass"
    blocking = any(f.severity in FAIL_SEVERITIES for f in findings)
    if blocking:
        status = "fail"

    return status, findings, evidence


def build_contract_output(
    status: str,
    findings: list[Finding],
    evidence: dict[str, str],
    duration_ms: int,
    repo_root: Path,
    exit_code: int,
) -> dict[str, Any]:
    findings_payload = [
        {
            "code": f.code,
            "severity": f.severity,
            "category": f.category,
            "message": f.message,
            "path": f.path,
            "line": f.line,
            "evidence_ref": f.evidence_ref,
        }
        for f in findings
    ]

    warnings_total = sum(1 for f in findings if f.severity == "low")
    stdout_blob = json.dumps({"status": status}, sort_keys=True)

    return {
        "adapter_result": {
            "status": status,
            "plugin_id": "P21",
            "adapter_id": "python-release-check",
            "tool": {
                "command": "python3",
                "version": "3",
            },
            "findings": findings_payload,
            "metrics": {
                "duration_ms": duration_ms,
                "findings_total": len(findings),
                "warnings_total": warnings_total,
            },
            "evidence": {
                "stdout_hash": hash_text(stdout_blob),
                "stderr_hash": hash_text(""),
                "report_hash": hash_text(json.dumps(findings_payload, sort_keys=True, default=str)),
                "report_path": None,
                "commit_sha": None,
                "repo_root": str(repo_root.resolve()),
            },
            "exit_code": exit_code,
        },
        "meta": {
            "mode": "ci_fast",
            "tool_invocation": {
                "command": "python3",
                "args": [
                    "tools/custom/release-check/release_readiness.py",
                    "--repo-root",
                    str(repo_root),
                ],
                "env_policy": {
                    "mode": "allowlist",
                    "allow_commands": [],
                },
            },
            "evidence": evidence,
            "errors": {
                "critical_findings": [f.code for f in findings if f.severity == "critical"],
                "high_findings": [f.code for f in findings if f.severity == "high"],
            },
        },
    }


def main() -> int:
    started = time.perf_counter_ns()
    try:
        args = parse_args()
        repo_root = Path(args.repo_root).resolve()
        if not repo_root.is_dir():
            raise RuntimeError(f"repo root not found: {repo_root}")

        status, findings, evidence = run_checks(repo_root)

        duration_ms = (time.perf_counter_ns() - started) // 1_000_000
        exit_code = 0 if status == "pass" else 101

        payload = build_contract_output(
            status=status,
            findings=findings,
            evidence=evidence,
            duration_ms=duration_ms,
            repo_root=repo_root,
            exit_code=exit_code,
        )
        output = json.dumps(payload, indent=2, sort_keys=True)
        print(output)

        # Compact machine-parsable line used by receipt regex.
        if status == "pass":
            print("P21_PASS")
            return exit_code

        print("P21_FAIL")
        return exit_code
    except Exception as exc:
        msg = str(exc)
        duration_ms = (time.perf_counter_ns() - started) // 1_000_000
        error_payload = {
            "adapter_result": {
                "status": "error",
                "plugin_id": "P21",
                "adapter_id": "python-release-check",
                "tool": {
                    "command": "python3",
                    "version": "3",
                },
                "findings": [
                    {
                        "code": "p21.internal.error",
                        "severity": "critical",
                        "category": "release.internal",
                        "message": msg,
                        "path": None,
                        "line": None,
                        "evidence_ref": "release.exception",
                    }
                ],
                "metrics": {
                    "duration_ms": duration_ms,
                    "findings_total": 1,
                    "warnings_total": 0,
                },
                "evidence": {
                    "stdout_hash": hash_text(msg),
                    "stderr_hash": hash_text(""),
                    "report_hash": hash_text(msg),
                    "report_path": None,
                    "commit_sha": None,
                },
            }
        }
        print(json.dumps(error_payload, indent=2, sort_keys=True))
        print("P21_ERROR")
        return 101


if __name__ == "__main__":
    raise SystemExit(main())
