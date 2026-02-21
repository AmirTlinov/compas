use ai_dx_mcp::{
    api::{DecisionStatus, GateKind},
    app::gate,
    repo::load_repo_config,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

fn repo_root_from_manifest_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/ai-dx-mcp -> repo root
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn write_file(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write file");
}

fn write_perf_baselines(repo_root: &Path, current: serde_json::Value, baseline: serde_json::Value) {
    let src = repo_root_from_manifest_dir();
    let script_src = src.join("scripts/perf_bench.py");
    std::fs::create_dir_all(repo_root.join("scripts")).expect("scripts dir");
    std::fs::copy(&script_src, repo_root.join("scripts/perf_bench.py")).expect("copy perf script");

    std::fs::create_dir_all(repo_root.join(".agents/mcp/compas/perf")).expect("create perf dir");
    std::fs::create_dir_all(repo_root.join(".agents/mcp/compas/baselines"))
        .expect("create baseline dir");
    write_file(
        repo_root.join(".agents/mcp/compas/perf/current.json"),
        &serde_json::to_string_pretty(&current).expect("serialize current"),
    );
    write_file(
        repo_root.join(".agents/mcp/compas/baselines/perf.json"),
        &serde_json::to_string_pretty(&baseline).expect("serialize baseline"),
    );
}

fn seed_perf_fixture(repo_root: &Path) {
    let src = repo_root_from_manifest_dir();
    let script_src = src.join("scripts/perf_bench.py");
    let tool_src = src.join("tools/p20/perf-bench/tool.toml");

    write_file(
        repo_root.join(".agents/mcp/compas/plugins/p20/plugin.toml"),
        r#"
[plugin]
id = "p20"
description = "Performance Regression Budget gate for runtime checks."
tool_import_globs = ["tools/p20/**/tool.toml"]

[gate]
ci_fast = ["perf-bench"]
ci = ["perf-bench"]
flagship = ["perf-bench"]
"#,
    );

    std::fs::create_dir_all(repo_root.join("tools/p20/perf-bench")).expect("plugin tool dir");
    std::fs::copy(&tool_src, repo_root.join("tools/p20/perf-bench/tool.toml"))
        .expect("copy perf tool");

    std::fs::create_dir_all(repo_root.join(".agents/mcp/compas")).expect("compas dir");
    write_file(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999

[proof]
require_witness = false
"#,
    );

    std::fs::create_dir_all(repo_root.join("scripts")).expect("scripts dir");
    std::fs::copy(&script_src, repo_root.join("scripts/perf_bench.py")).expect("copy perf script");
}

fn run_perf_bench(repo_root: &Path, baseline_rel: &str, current_rel: &str) -> (i32, Value) {
    let output = Command::new("python3")
        .current_dir(repo_root)
        .args([
            "scripts/perf_bench.py",
            "--baseline",
            baseline_rel,
            "--current",
            current_rel,
        ])
        .output()
        .expect("run perf_bench");
    let status = output.status.code().unwrap_or(-1);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("perf_bench JSON payload");
    (status, payload)
}

fn perf_metric_json(
    p95: f64,
    p99: f64,
    memory: f64,
    error_rate: f64,
) -> (serde_json::Value, serde_json::Value) {
    let baseline = serde_json::json!({
        "version": 1,
        "metrics": {
            "api_request_p95_ms": {
                "value": 120.0,
                "max_delta_pct": 8.0,
                "max_delta_abs": 12.0,
                "higher_is_worse": true,
                "severity": "medium",
            },
            "api_request_p99_ms": {
                "value": 280.0,
                "max_delta_pct": 10.0,
                "max_delta_abs": 24.0,
                "higher_is_worse": true,
                "severity": "high",
            },
            "memory_peak_mb": {
                "value": 640.0,
                "max_delta_pct": 12.0,
                "max_delta_abs": 96.0,
                "higher_is_worse": true,
                "severity": "low",
            },
            "error_rate_pct": {
                "value": 0.08,
                "max_delta_pct": 25.0,
                "max_delta_abs": 0.08,
                "higher_is_worse": true,
                "severity": "critical",
            },
        }
    });
    let current = serde_json::json!({
        "version": 1,
        "metrics": {
            "api_request_p95_ms": {"value": p95},
            "api_request_p99_ms": {"value": p99},
            "memory_peak_mb": {"value": memory},
            "error_rate_pct": {"value": error_rate},
        },
    });
    (baseline, current)
}

#[test]
fn p20_perf_bench_loads_imported_tool_and_gate_binding() {
    let dir = tempdir().expect("temp repo");
    seed_perf_fixture(dir.path());
    write_perf_baselines(
        dir.path(),
        serde_json::json!({"version": 1, "metrics": {}}),
        serde_json::json!({"version": 1, "metrics": {}}),
    );
    let cfg = load_repo_config(dir.path()).expect("load repo config");
    assert!(
        cfg.tools.contains_key("perf-bench"),
        "imported perf-bench tool must exist"
    );
    assert!(
        cfg.gate.ci_fast.iter().any(|id| id == "perf-bench"),
        "p20 plugin must wire ci_fast tool"
    );
    assert!(
        cfg.quality_contract.is_some(),
        "quality contract must be discoverable"
    );
}

#[test]
fn perf_bench_script_passes_when_within_budgets() {
    let dir = tempdir().expect("temp repo");
    let (baseline, current) = perf_metric_json(120.0, 280.0, 640.0, 0.08);
    write_perf_baselines(dir.path(), current, baseline);
    let (exit_code, payload) = run_perf_bench(
        dir.path(),
        ".agents/mcp/compas/baselines/perf.json",
        ".agents/mcp/compas/perf/current.json",
    );
    assert_eq!(exit_code, 0, "pass path must exit zero");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("pass"));
    assert_eq!(
        payload
            .get("summary")
            .and_then(|v| v.get("failures"))
            .and_then(Value::as_u64),
        Some(0)
    );
}

#[test]
fn perf_bench_script_blocks_regressions() {
    let dir = tempdir().expect("temp repo");
    let (baseline, current) = perf_metric_json(140.0, 280.0, 640.0, 0.08);
    write_perf_baselines(dir.path(), current, baseline);
    let (exit_code, payload) = run_perf_bench(
        dir.path(),
        ".agents/mcp/compas/baselines/perf.json",
        ".agents/mcp/compas/perf/current.json",
    );
    assert_eq!(exit_code, 1, "regression must be non-zero");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("fail"));
    let failures = payload
        .get("summary")
        .and_then(|v| v.get("findings_total"))
        .and_then(Value::as_u64)
        .expect("findings_total");
    assert!(failures > 0, "should produce blocking findings");
    assert!(
        payload
            .get("findings")
            .and_then(|v| v.as_array())
            .expect("findings")
            .iter()
            .any(|f| f.get("code").and_then(Value::as_str) == Some("P20.metric_regression")),
        "regression finding must be surfaced"
    );
}

#[test]
fn perf_bench_script_reports_schema_error() {
    let dir = tempdir().expect("temp repo");
    let baseline = serde_json::json!({
        "version": 1,
        "metrics": {
            "api_request_p95_ms": {
                "value": 120.0,
                "max_delta_pct": 8.0,
                "max_delta_abs": 12.0,
                "higher_is_worse": true,
                "severity": "unknown",
            }
        }
    });
    let current = serde_json::json!({
        "version": 1,
        "metrics": {
            "api_request_p95_ms": {"value": 120.0}
        }
    });
    write_perf_baselines(dir.path(), current, baseline);
    let (exit_code, payload) = run_perf_bench(
        dir.path(),
        ".agents/mcp/compas/baselines/perf.json",
        ".agents/mcp/compas/perf/current.json",
    );
    assert_eq!(exit_code, 1, "schema errors must be non-zero");
    assert_eq!(payload.get("status").and_then(Value::as_str), Some("error"));
    assert_eq!(
        payload
            .get("findings")
            .and_then(|v| v.as_array())
            .expect("findings")
            .iter()
            .any(|f| f.get("code").and_then(Value::as_str) == Some("P20.schema_error")),
        true
    );
}

#[tokio::test]
async fn p20_gate_blocks_on_regression() {
    let dir = tempdir().expect("temp repo");
    seed_perf_fixture(dir.path());
    let (baseline, current) = perf_metric_json(120.0, 400.0, 640.0, 0.08);
    write_perf_baselines(dir.path(), current, baseline);

    let out = gate(
        &dir.path().to_string_lossy(),
        GateKind::CiFast,
        false,
        false,
    )
    .await;
    assert!(!out.ok, "regression must block gate");
    assert_eq!(out.receipts.len(), 1, "expected one gate receipt");
    assert!(
        !out.receipts[0].success,
        "regressed run must be non-zero exit"
    );
    let verdict = out.verdict.clone().expect("verdict");
    assert_eq!(verdict.decision.status, DecisionStatus::Blocked);
    assert!(
        verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.tool_failed.perf-bench"),
        "block reason must reference failing perf tool"
    );
}

#[tokio::test]
async fn p20_gate_passes_within_budget() {
    let dir = tempdir().expect("temp repo");
    seed_perf_fixture(dir.path());
    let (baseline, current) = perf_metric_json(120.0, 280.0, 640.0, 0.08);
    write_perf_baselines(dir.path(), current, baseline);

    let out = gate(
        &dir.path().to_string_lossy(),
        GateKind::CiFast,
        false,
        false,
    )
    .await;
    assert!(out.ok, "in-budget metrics must pass gate");
    assert_eq!(out.receipts.len(), 1, "expected one gate receipt");
    assert_eq!(out.receipts[0].tool_id, "perf-bench");
    assert!(
        out.receipts[0].success,
        "in-budget metrics must produce successful tool receipt"
    );
    assert_eq!(out.receipts[0].exit_code, Some(0));
}
