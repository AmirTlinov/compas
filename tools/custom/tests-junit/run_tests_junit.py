#!/usr/bin/env python3
"""Run repo tests and emit a merged JUnit XML report."""

from __future__ import annotations

import re
import subprocess
import sys
import time
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable
from xml.etree import ElementTree as ET


ROOT = Path.cwd()
OUT_DIR = ROOT / "artifacts" / "tests"
OUT_DIR.mkdir(parents=True, exist_ok=True)
OUT_JUNIT = OUT_DIR / "p14-junit.xml"


@dataclass
class CaseResult:
    name: str
    classname: str
    status: str
    time: str = "0"
    message: str | None = None
    details: str | None = None


@dataclass
class SuiteResult:
    name: str
    cases: list[CaseResult]
    time: float = 0.0

    @property
    def counts(self) -> tuple[int, int, int, int]:
        total = len(self.cases)
        failed = sum(1 for c in self.cases if c.status == "failure")
        errors = sum(1 for c in self.cases if c.status == "error")
        skipped = sum(1 for c in self.cases if c.status == "skipped")
        return total, failed, errors, skipped


def run_command(cmd: list[str], cwd: Path, env: dict[str, str] | None = None) -> tuple[int, str, str, float]:
    start = time.monotonic()
    run_env = None if env is None else {**os.environ, **env}
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            env=run_env,
        )
    except FileNotFoundError as exc:
        duration = time.monotonic() - start
        return 127, "", str(exc), duration

    duration = time.monotonic() - start
    return proc.returncode, proc.stdout or "", proc.stderr or "", duration


def parse_int(token: str) -> int:
    value = re.findall(r"\d+", token)
    return int(value[0]) if value else 0


def parse_cargo_summary(text: str) -> tuple[int, int, int, int, int, str]:
    # Example:
    # test result: ok. 12 passed; 0 failed; 0 ignored; 1 measured; 0 filtered out; finished in 0.00s
    summary_line = ""
    for line in text.splitlines():
        if line.startswith("test result:"):
            summary_line = line
            break

    if not summary_line:
        return 0, 0, 0, 0, 0, "missing"

    m = re.search(
        r"test result:\s+(ok|FAILED)\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;\s+(\d+)\s+ignored;\s+(\d+)\s+measured;\s+(\d+)\s+filtered out;",
        summary_line,
    )
    if m:
        status = "failed" if m.group(1) == "FAILED" else "passed"
        return (
            int(m.group(2)),
            int(m.group(3)),
            int(m.group(4)),
            int(m.group(5)),
            int(m.group(6)),
            status,
        )

    # Fallback to token parse for non-standard formatting.
    tokens = summary_line.replace(";", "").split()
    passed = failed = ignored = measured = filtered = 0
    for idx, tok in enumerate(tokens):
        if tok == "passed":
            passed = parse_int(tokens[idx - 1])
        elif tok == "failed":
            failed = parse_int(tokens[idx - 1])
        elif tok == "ignored":
            ignored = parse_int(tokens[idx - 1])
        elif tok == "measured":
            measured = parse_int(tokens[idx - 1])
        elif tok == "filtered":
            filtered = parse_int(tokens[idx - 1])

    status = "failed" if "FAILED" in summary_line else "passed"
    return passed, failed, ignored, measured, filtered, status


def run_cargo_suite() -> SuiteResult:
    code, stdout, stderr, duration = run_command(
        ["cargo", "test", "-p", "ai-dx-mcp"],
        ROOT,
        {"CARGO_TERM_COLOR": "never"},
    )
    combined = stdout + "\n" + stderr
    passed, failed, ignored, measured, filtered, summary_status = parse_cargo_summary(combined)

    if code != 0:
        return SuiteResult(
            name="cargo-rust",
            time=duration,
            cases=[
                CaseResult(
                    name="ai-dx-mcp-cargo-test",
                    classname="p14.cargo",
                    status="failure",
                    message="cargo test failed",
                    details=combined.strip(),
                )
            ],
        )

    if summary_status == "missing":
        return SuiteResult(
            name="cargo-rust",
            time=duration,
            cases=[
                CaseResult(
                    name="cargo-summary-missing",
                    classname="p14.cargo",
                    status="error",
                    message="Unable to parse cargo test summary",
                    details=combined[-4000:],
                )
            ],
        )

    if passed == 0 and filtered == 0 and ignored == 0 and measured == 0 and failed == 0:
        return SuiteResult(
            name="cargo-rust",
            time=duration,
            cases=[
                CaseResult(
                    name="ai-dx-mcp-cargo-test",
                    classname="p14.cargo",
                    status="failure",
                    message="No Rust tests discovered for ai-dx-mcp",
                    details=combined.strip(),
                )
            ],
        )

    if failed > 0:
        return SuiteResult(
            name="cargo-rust",
            time=duration,
            cases=[
                CaseResult(
                    name="ai-dx-mcp-cargo-test",
                    classname="p14.cargo",
                    status="failure",
                    message="cargo test reported failures",
                    details=combined.strip(),
                )
            ],
        )

    return SuiteResult(
        name="cargo-rust",
        time=duration,
        cases=[
            CaseResult(
                name="ai-dx-mcp-cargo-test",
                classname="p14.cargo",
                status="passed",
                time=f"{duration:.3f}",
                message=f"passed={passed}, ignored={ignored}, measured={measured}, filtered={filtered}",
            )
        ],
    )


def parse_pytest_xml(path: Path) -> SuiteResult:
    try:
        root = ET.parse(path).getroot()
    except Exception as exc:  # pragma: no cover - parsing fallback
        return SuiteResult(
            name="pytest-python",
            cases=[
                CaseResult(
                    name="pytest-junit-parse",
                    classname="p14.pytest",
                    status="error",
                    message="Failed to parse pytest junit file",
                    details=str(exc),
                )
            ],
        )

    name = root.attrib.get("name", "pytest")
    cases: list[CaseResult] = []
    for tc in root.findall(".//testcase"):
        failure_node = tc.find("failure")
        error_node = tc.find("error")
        skipped_node = tc.find("skipped")
        case_name = tc.attrib.get("name", "unknown")
        classname = tc.attrib.get("classname", "p14.pytest")
        duration = tc.attrib.get("time", "0") or "0"

        if failure_node is not None:
            message = failure_node.attrib.get("message") or "pytest failure"
            details = (failure_node.text or "").strip()
            status = "failure"
        elif error_node is not None:
            message = error_node.attrib.get("message") or "pytest error"
            details = (error_node.text or "").strip()
            status = "error"
        elif skipped_node is not None:
            message = skipped_node.attrib.get("message") or "skipped"
            details = (skipped_node.text or "").strip()
            status = "skipped"
        else:
            message = None
            details = None
            status = "passed"

        cases.append(
            CaseResult(
                name=case_name,
                classname=classname,
                status=status,
                time=duration,
                message=message,
                details=details,
            )
        )

    if not cases:
        return SuiteResult(
            name="pytest-python",
            cases=[
                CaseResult(
                    name="pytest-empty-junit",
                    classname="p14.pytest",
                    status="skipped",
                    message="Pytest produced an empty JUnit report",
                )
            ],
        )

    return SuiteResult(name=name, cases=cases)


def run_pytest_suite() -> SuiteResult:
    junit_tmp = OUT_DIR / "pytest-junit.xml"
    code, stdout, stderr, duration = run_command(
        ["pytest", "--junitxml", str(junit_tmp), "-q"],
        ROOT,
        {"PYTHONUNBUFFERED": "1"},
    )

    if code == 5:
        return SuiteResult(
            name="pytest-python",
            time=duration,
            cases=[
                CaseResult(
                    name="pytest-no-tests",
                    classname="p14.pytest",
                    status="skipped",
                    message="No python tests discovered",
                )
            ],
        )

    if code != 0:
        return SuiteResult(
            name="pytest-python",
            time=duration,
            cases=[
                CaseResult(
                    name="pytest-run",
                    classname="p14.pytest",
                    status="failure",
                    message="pytest exited with non-zero code",
                    details=(stdout + "\n" + stderr).strip(),
                )
            ],
        )

    if not junit_tmp.exists():
        return SuiteResult(
            name="pytest-python",
            time=duration,
            cases=[
                CaseResult(
                    name="pytest-junit-output-missing",
                    classname="p14.pytest",
                    status="error",
                    message="pytest did not emit junitxml file",
                )
            ],
        )

    suite = parse_pytest_xml(junit_tmp)
    suite.time = duration
    return suite


def merge_suites(suites: Iterable[SuiteResult]) -> bytes:
    suites = list(suites)
    root = ET.Element("testsuites")
    totals = {"tests": 0, "failures": 0, "errors": 0, "skipped": 0}

    for suite in suites:
        tests, failures, errors, skipped = suite.counts
        totals["tests"] += tests
        totals["failures"] += failures
        totals["errors"] += errors
        totals["skipped"] += skipped

        suite_attrs = {
            "name": suite.name,
            "tests": str(tests),
            "failures": str(failures),
            "errors": str(errors),
            "skipped": str(skipped),
            "time": f"{suite.time:.6f}",
        }
        sx = ET.SubElement(root, "testsuite", suite_attrs)

        for case in suite.cases:
            cx = ET.SubElement(
                sx,
                "testcase",
                {"name": case.name, "classname": case.classname, "time": case.time},
            )
            if case.status == "failure":
                ET.SubElement(
                    cx,
                    "failure",
                    {"message": case.message or "failed", "type": "AssertionError"},
                ).text = case.details or ""
            elif case.status == "error":
                ET.SubElement(
                    cx,
                    "error",
                    {"message": case.message or "error", "type": "RuntimeError"},
                ).text = case.details or ""
            elif case.status == "skipped":
                ET.SubElement(cx, "skipped", {"message": case.message or "skipped"}).text = (
                    case.details or ""
                )

    root.attrib.update(
        {
            "name": "p14-tests",
            "tests": str(totals["tests"]),
            "failures": str(totals["failures"]),
            "errors": str(totals["errors"]),
            "skipped": str(totals["skipped"]),
            "time": f"{sum(s.time for s in suites):.6f}",
        }
    )
    return ET.tostring(root, encoding="utf-8", xml_declaration=True)


def normalize_exit(suites: Iterable[SuiteResult]) -> int:
    suites = list(suites)
    total_non_skipped = 0
    total_failures = 0
    total_errors = 0

    for suite in suites:
        total, failed, errors, skipped = suite.counts
        if skipped < len(suite.cases):
            total_non_skipped += total - skipped
        total_failures += failed
        total_errors += errors

    if total_non_skipped == 0:
        return 1
    if total_failures > 0 or total_errors > 0:
        return 1
    return 0


def main() -> int:
    suites = [run_cargo_suite(), run_pytest_suite()]
    OUT_JUNIT.write_bytes(merge_suites(suites))

    total_tests = sum(s.counts[0] for s in suites)
    total_failed = sum(s.counts[1] for s in suites)
    total_errors = sum(s.counts[2] for s in suites)
    total_skipped = sum(s.counts[3] for s in suites)
    total_passed = max(0, total_tests - total_failed - total_errors - total_skipped)

    print(
        "P14 tests-junit: total={} passed={} failed={} errors={} skipped={} report={}".format(
            total_tests,
            total_passed,
            total_failed,
            total_errors,
            total_skipped,
            OUT_JUNIT,
        )
    )
    return normalize_exit(suites)


if __name__ == "__main__":
    sys.exit(main())
