#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple


ALLOWED_SEVERITIES = {"low", "medium", "high", "critical"}


class BenchmarkError(ValueError):
    pass


@dataclass(frozen=True)
class MetricBudget:
    value: float
    max_delta_pct: float
    max_delta_abs: float
    higher_is_worse: bool
    severity: str


@dataclass(frozen=True)
class MetricSample:
    value: float
    severity: Optional[str] = None


def _must_be_object(value: Any, label: str) -> Dict[str, Any]:
    if not isinstance(value, dict):
        raise BenchmarkError(f"{label} must be an object")
    return value


def _must_be_nonempty_str(value: Any, label: str) -> str:
    if not isinstance(value, str):
        raise BenchmarkError(f"{label} must be a string")
    value = value.strip()
    if not value:
        raise BenchmarkError(f"{label} must not be empty")
    return value


def _must_be_number(value: Any, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise BenchmarkError(f"{label} must be a finite number")
    if isinstance(value, bool):
        raise BenchmarkError(f"{label} must be a number")
    if not math.isfinite(value):
        raise BenchmarkError(f"{label} must be finite")
    return float(value)


def _must_be_bool(value: Any, label: str) -> bool:
    if not isinstance(value, bool):
        raise BenchmarkError(f"{label} must be boolean")
    return value


def _validate_metric_name(name: str) -> str:
    metric_re = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
    if not metric_re.match(name):
        raise BenchmarkError(f"invalid metric name '{name}'")
    return name


def _load_payload(path: Path) -> Dict[str, Any]:
    if not path.is_file():
        raise BenchmarkError(f"file does not exist: {path}")

    try:
        text = path.read_text(encoding="utf-8")
        payload = json.loads(text)
    except json.JSONDecodeError as exc:
        raise BenchmarkError(f"invalid JSON in {path}: {exc}") from exc

    data = _must_be_object(payload, f"{path}")
    allowed_keys = {"version", "metrics"}
    unknown = set(data.keys()) - allowed_keys
    if unknown:
        raise BenchmarkError(f"{path} contains unknown top-level keys: {sorted(unknown)}")

    version = data.get("version")
    if version != 1:
        raise BenchmarkError(f"{path} must use schema version 1")

    return data


def _parse_baseline(data: Dict[str, Any], path: Path) -> Dict[str, MetricBudget]:
    metrics_obj = _must_be_object(data.get("metrics"), f"{path}.metrics")
    if not metrics_obj:
        raise BenchmarkError(f"{path}.metrics must include at least one metric")

    out: Dict[str, MetricBudget] = {}

    for raw_name, raw_metric in metrics_obj.items():
        name = _must_be_nonempty_str(raw_name, f"{path}.metrics key")
        _validate_metric_name(name)

        metric = _must_be_object(raw_metric, f"{path}.metrics[{name}]")
        required = {"value", "max_delta_pct", "max_delta_abs", "higher_is_worse", "severity"}
        unknown = set(metric.keys()) - required
        if unknown:
            raise BenchmarkError(
                f"{path}.metrics[{name}] contains unknown keys: {sorted(unknown)}"
            )
        missing = required - set(metric.keys())
        if missing:
            raise BenchmarkError(
                f"{path}.metrics[{name}] missing keys: {sorted(missing)}"
            )

        value = _must_be_number(metric.get("value"), f"{path}.metrics[{name}].value")
        max_delta_pct = _must_be_number(
            metric.get("max_delta_pct"),
            f"{path}.metrics[{name}].max_delta_pct",
        )
        max_delta_abs = _must_be_number(
            metric.get("max_delta_abs"),
            f"{path}.metrics[{name}].max_delta_abs",
        )
        higher_is_worse = _must_be_bool(
            metric.get("higher_is_worse"),
            f"{path}.metrics[{name}].higher_is_worse",
        )
        severity = _must_be_nonempty_str(metric.get("severity"), f"{path}.metrics[{name}].severity").lower()

        if severity not in ALLOWED_SEVERITIES:
            raise BenchmarkError(
                f"{path}.metrics[{name}].severity must be one of {sorted(ALLOWED_SEVERITIES)}"
            )
        if max_delta_pct < 0:
            raise BenchmarkError(f"{path}.metrics[{name}].max_delta_pct must be >= 0")
        if max_delta_abs < 0:
            raise BenchmarkError(f"{path}.metrics[{name}].max_delta_abs must be >= 0")

        out[name] = MetricBudget(
            value=value,
            max_delta_pct=max_delta_pct,
            max_delta_abs=max_delta_abs,
            higher_is_worse=higher_is_worse,
            severity=severity,
        )

    return out


def _parse_current(data: Dict[str, Any], path: Path) -> Dict[str, MetricSample]:
    metrics_obj = _must_be_object(data.get("metrics"), f"{path}.metrics")
    if not metrics_obj:
        raise BenchmarkError(f"{path}.metrics must include at least one metric")

    out: Dict[str, MetricSample] = {}
    for raw_name, raw_metric in metrics_obj.items():
        name = _must_be_nonempty_str(raw_name, f"{path}.metrics key")
        _validate_metric_name(name)

        metric = _must_be_object(raw_metric, f"{path}.metrics[{name}]")
        unknown = set(metric.keys()) - {"value"}
        if unknown:
            raise BenchmarkError(
                f"{path}.metrics[{name}] contains unknown keys: {sorted(unknown)}"
            )
        if "value" not in metric:
            raise BenchmarkError(f"{path}.metrics[{name}] missing value")

        value = _must_be_number(metric.get("value"), f"{path}.metrics[{name}].value")
        out[name] = MetricSample(value=value)

    return out


def _compare_metric(
    name: str,
    base: MetricBudget,
    current: MetricSample,
) -> Tuple[bool, Dict[str, Any], Optional[Dict[str, Any]]]:
    delta = current.value - base.value
    regression_delta = delta if base.higher_is_worse else -delta
    regression = regression_delta > 0

    delta_pct = None
    if regression and base.value != 0:
        delta_pct = (regression_delta / abs(base.value)) * 100
    elif regression and base.value == 0 and base.max_delta_pct > 0:
        delta_pct = float("inf")

    summary = {
        "baseline": base.value,
        "current": current.value,
        "delta_abs": regression_delta,
        "delta_pct": delta_pct,
        "higher_is_worse": base.higher_is_worse,
    }

    finding = None
    failed = False
    if regression:
        if base.max_delta_abs >= 0 and regression_delta > base.max_delta_abs + 1e-12:
            failed = True
        elif delta_pct is not None and base.max_delta_pct >= 0 and delta_pct > base.max_delta_pct:
            failed = True

    if regression and not failed:
        failed = False

    if failed:
        budget_bits = [
            f"max_delta_pct={base.max_delta_pct}",
            f"max_delta_abs={base.max_delta_abs}",
        ]
        message = (
            f"metric {name} regressed: baseline={base.value:.12g}, current={current.value:.12g}, "
            f"delta={delta:.12g}, delta_pct={delta_pct if delta_pct is not None else 'n/a'}, "
            f"required {'; '.join(budget_bits)}"
        )
        finding = {
            "code": "P20.metric_regression",
            "severity": base.severity,
            "metric": name,
            "message": message,
            "delta": regression_delta,
            "delta_pct": delta_pct,
            "limit_pct": base.max_delta_pct,
            "limit_abs": base.max_delta_abs,
        }

    return failed, summary, finding


def run_compare(baseline_path: Path, current_path: Path) -> Dict[str, Any]:
    baseline_data = _load_payload(baseline_path)
    current_data = _load_payload(current_path)

    baseline = _parse_baseline(baseline_data, baseline_path)
    current = _parse_current(current_data, current_path)

    findings = []
    metrics: Dict[str, Any] = {}
    error = None

    for name in sorted(baseline.keys()):
        if name not in current:
            metrics[name] = {
                "status": "missing_current",
                "baseline": baseline[name].value,
            }
            findings.append(
                {
                    "code": "P20.missing_metric",
                    "severity": baseline[name].severity,
                    "metric": name,
                    "message": f"current payload missing baseline metric '{name}'",
                }
            )
            continue

        failed, summary, finding = _compare_metric(name, baseline[name], current[name])
        if finding is not None:
            findings.append(finding)
        metrics[name] = {
            "baseline": summary["baseline"],
            "current": summary["current"],
            "delta_abs": summary["delta_abs"],
            "delta_pct": summary["delta_pct"],
            "status": "fail" if failed else "pass",
            "higher_is_worse": summary["higher_is_worse"],
        }

    for name in sorted(set(current.keys()) - set(baseline.keys())):
        metrics[name] = {
            "status": "extra_current",
            "current": current[name].value,
        }
        findings.append(
            {
                "code": "P20.extra_metric",
                "severity": "medium",
                "metric": name,
                "message": f"current payload includes unmapped metric '{name}'",
            }
        )

    if any(item["status"] == "missing_current" for item in metrics.values()):
        error = "current baseline metric coverage mismatch"

    if any(item["status"] == "extra_current" for item in metrics.values()):
        error = "current payload has unmapped metrics"

    status = "pass"
    if findings:
        status = "error" if any(f["code"].startswith("P20.missing_metric") or f["code"].startswith("P20.extra_metric") for f in findings) else "fail"

    return {
        "status": status,
        "summary": {
            "metrics_total": len(metrics),
            "findings_total": len(findings),
            "failures": sum(1 for f in findings if f["code"] == "P20.metric_regression"),
            "errors": 1 if error else 0,
        },
        "error": error,
        "findings": findings,
        "metrics": metrics,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Performance budget comparator for P20")
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--current", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    baseline = Path(args.baseline)
    current = Path(args.current)

    try:
        result = run_compare(baseline, current)
    except BenchmarkError as exc:
        payload = {
            "status": "error",
            "summary": {
                "metrics_total": 0,
                "findings_total": 1,
                "failures": 0,
                "errors": 1,
            },
            "error": str(exc),
            "findings": [
                {
                    "code": "P20.schema_error",
                    "severity": "critical",
                    "message": str(exc),
                }
            ],
            "metrics": {},
        }
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 1

    print(json.dumps(result, indent=2, sort_keys=True))

    if result["status"] == "pass":
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
