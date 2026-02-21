#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from hashlib import sha256
from pathlib import Path
from typing import Dict, List, Set, Tuple


SCRIPT_VERSION = "1.0.0"

MAX_FILE_BYTES = 512 * 1024
MAX_REPORT_FINDINGS = 5000
MAX_LINES_PER_FILE = 2000
MAX_FILES = 20000

INCLUDE_EXTENSIONS = {
    ".rs",
    ".py",
    ".js",
    ".jsx",
    ".ts",
    ".tsx",
    ".java",
    ".go",
    ".kt",
    ".swift",
    ".cs",
    ".json",
    ".toml",
    ".yaml",
    ".yml",
    ".sh",
    ".bash",
    ".ps1",
}

EXCLUDE_DIRS = {
    ".git",
    ".idea",
    ".vscode",
    "target",
    "dist",
    "build",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".cache",
    "node_modules",
    ".venv",
    "vendor",
}


LOG_HINT_RE = re.compile(
    r"(?i)(?:\blogger\b|\blogging\b|\btracing\b|\bslog\b|\bwinston\b|\blog4j\b|\bloguru\b|\bbunyan\b|"
    r"\bprintln!|\beprintln!|\bdebug!|\binfo!|\bwarn!|\berror!|\btrace!|\bdebugf!|\binfof!|\bwarnf!|\berrorf!|"
    r"\bconsole\.[a-zA-Z_]+\(|\bprintf?\(|\bSystem\.out\.println\(|\bConsole\.WriteLine\(|\bfmt\.[A-Za-z0-9_]+!\(|\blog\.[A-Za-z0-9_]+\(|\bsys\.log\.[A-Za-z0-9_]+\()",
)


@dataclass(frozen=True)
class Pattern:
    code: str
    severity: str
    category: str
    regex: re.Pattern[str]
    require_log_context: bool


def compile_pattern(pattern: str) -> re.Pattern[str]:
    return re.compile(pattern, re.IGNORECASE)


PATTERNS: Tuple[Pattern, ...] = (
    Pattern("PII-SECRET-BLOCK", "critical", "credential", compile_pattern(r"-+BEGIN [A-Z ]*PRIVATE KEY-+"), False),
    Pattern("PII-AWS-KEY", "high", "credential", compile_pattern(r"\bAKIA[0-9A-Z]{16}\b"), False),
    Pattern("PII-JWT", "high", "credential", compile_pattern(r"eyJ[0-9a-zA-Z_-]{10,}\.[0-9a-zA-Z_-]{10,}\.[0-9a-zA-Z_-]{10,}"), False),
    Pattern("PII-API-KEY", "high", "credential", compile_pattern(r"\b(?:api[_-]?key|apikey)\b[^\n]{0,60}[=:\s][\"']?([A-Za-z0-9_./+=-]{16,})[\"']?"), True),
    Pattern("PII-SECRET", "high", "credential", compile_pattern(r"\b(?:secret|private[_-]?key|client[_-]?secret)\b[^\n]{0,60}[=:\s][\"']?([A-Za-z0-9_./+=-]{12,})[\"']?"), True),
    Pattern("PII-TOKEN", "high", "credential", compile_pattern(r"\b(?:auth(?:entication)?[_-]?token|access[_-]?token|oauth[_-]?token)\b[^\n]{0,60}[=:\s][\"']?[A-Za-z0-9._~/+\-=]{16,}[\"']?"), True),
    Pattern("PII-PASSWORD", "medium", "credential", compile_pattern(r"\b(?:password|passwd|passphrase)\b[^\n]{0,40}[=:\s][\"']?.{4,}[\"']?"), True),
    Pattern("PII-EMAIL", "medium", "pii", compile_pattern(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"), True),
    Pattern("PII-PHONE", "low", "pii", compile_pattern(r"\b(?:\+\d{1,3}[\s-]?)?(?:\(\d{3}\)|\d{3})[\s.-]?\d{3}[\s.-]?\d{4}\b"), True),
    Pattern("PII-SSN", "high", "pii", compile_pattern(r"\b\d{3}-\d{2}-\d{4}\b"), True),
)


def hash_bytes(payload: bytes) -> str:
    return sha256(payload).hexdigest()[:12]


def is_binary(data: bytes) -> bool:
    return b"\x00" in data


def git_commit_sha(repo_root: Path) -> str:
    try:
        cp = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            check=False,
            text=True,
            capture_output=True,
            timeout=2.0,
        )
        if cp.returncode == 0:
            return cp.stdout.strip().splitlines()[0]
    except Exception:
        return "unknown"
    return "unknown"


def should_skip_dir(name: str) -> bool:
    if name in EXCLUDE_DIRS:
        return True
    return name.startswith(".") and name != ".github"


def iter_scan_files(repo_root: Path) -> List[Path]:
    files: List[Path] = []
    for dirpath, dirnames, filenames in os.walk(repo_root):
        dirnames[:] = [d for d in dirnames if not should_skip_dir(d)]
        for filename in filenames:
            path = Path(dirpath) / filename
            if path.suffix.lower() not in INCLUDE_EXTENSIONS:
                continue
            if any(part in EXCLUDE_DIRS for part in path.parts):
                continue
            if path.name.startswith(".") and path.name != ".github":
                continue
            try:
                if path.is_file() and path.stat().st_size <= MAX_FILE_BYTES:
                    files.append(path)
            except OSError:
                continue
    files.sort()
    return files[:MAX_FILES]


def scan_file(
    path: Path, root: Path, findings: List[Dict], seen: Set[Tuple[str, int, str]]
) -> Tuple[int, bool]:
    try:
        raw = path.read_bytes()
    except OSError:
        return 0, True

    if not raw.strip():
        return 0, True

    if is_binary(raw):
        return 0, True

    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        text = raw.decode("utf-8", errors="replace")

    rel = str(path.relative_to(root))
    added = 0

    for i, line in enumerate(text.splitlines(), 1):
        if i > MAX_LINES_PER_FILE:
            break
        if not line.strip():
            continue

        in_log_ctx = LOG_HINT_RE.search(line) is not None
        for p in PATTERNS:
            if p.require_log_context and not in_log_ctx:
                continue
            if not p.regex.search(line):
                continue

            key = (rel, i, p.code)
            if key in seen:
                continue
            seen.add(key)
            findings.append(
                {
                    "code": p.code,
                    "severity": p.severity,
                    "category": p.category,
                    "message": f"Potential {p.category} leak ({p.code})",
                    "path": rel,
                    "line": i,
                    "evidence_ref": f"{rel}:{i}",
                }
            )
            added += 1
            if added + len(findings) >= MAX_REPORT_FINDINGS:
                return added, False
    return added, False


def build_report(findings: List[Dict], duration_ms: int, scan_stats: Dict[str, int], repo_root: Path) -> Dict:
    severity_rank = {"critical": 4, "high": 3, "medium": 2, "low": 1}
    ordered = sorted(
        findings,
        key=lambda it: (
            -severity_rank.get(it["severity"], 0),
            it["path"],
            it["line"],
            it["code"],
        ),
    )

    return {
        "adapter_result": {
            "status": "pass" if not findings else "fail",
            "plugin_id": "p18",
            "adapter_id": "log-scan-v1",
            "tool": {
                "command": "python3",
                "version": platform.python_version(),
            },
            "findings": ordered,
            "metrics": {
                "duration_ms": duration_ms,
                "findings_total": len(findings),
                "warnings_total": sum(1 for f in findings if f["severity"] in {"low", "medium"}),
            },
            "evidence": {
                "stdout_hash": "",
                "stderr_hash": "",
                "report_hash": None,
                "report_path": None,
                "commit_sha": git_commit_sha(repo_root),
            },
            "scan_stats": scan_stats,
        }
    }


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Scan repository for logging-related PII or secret leaks")
    p.add_argument("--repo-root", default=".", dest="repo_root")
    p.add_argument("--max-findings", type=int, default=MAX_REPORT_FINDINGS)
    return p.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(args.repo_root).resolve()
    if not root.is_dir():
        print("repo-root is not a directory", file=sys.stderr)
        return 2

    max_findings = max(1, args.max_findings)
    files = iter_scan_files(root)

    findings: List[Dict] = []
    seen: Set[Tuple[str, int, str]] = set()
    scanned_files = 0
    skipped_files = 0

    start = time.time()
    for path in files:
        added, skipped = scan_file(path, root, findings, seen)
        if skipped:
            skipped_files += 1
        scanned_files += 1
        if added and len(findings) >= max_findings:
            break
    duration_ms = int((time.time() - start) * 1000)

    report = build_report(
        findings,
        duration_ms=duration_ms,
        scan_stats={
            "files": len(files),
            "scanned_files": scanned_files,
            "skipped_files": skipped_files,
        },
        repo_root=root,
    )

    status = report["adapter_result"]["status"]
    findings_count = len(findings)
    if status != "pass":
        status_text = f"status={status} findings={findings_count}"
    else:
        status_text = "status=pass"

    stdout_payload = json.dumps(report, ensure_ascii=False, indent=2)
    stdout_hash = hash_bytes(stdout_payload.encode("utf-8"))
    report["adapter_result"]["evidence"]["stdout_hash"] = stdout_hash

    stderr_text = f"PII_SCAN_RESULT {status_text} adapter={report['adapter_result']['adapter_id']}"
    stderr_hash = hash_bytes((stderr_text + "\n").encode("utf-8"))
    report["adapter_result"]["evidence"]["stderr_hash"] = stderr_hash

    final_payload = json.dumps(report, ensure_ascii=False, indent=2)
    report["adapter_result"]["evidence"]["report_hash"] = hash_bytes(final_payload.encode("utf-8"))

    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(stderr_text, file=sys.stderr)

    if status == "pass":
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
