use ai_dx_mcp::{api::GateKind, app::gate};

fn write_plugin(repo: &std::path::Path, gate_ci_fast: &[&str]) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default"))
        .expect("mkdir plugin dir");

    let gate_items = gate_ci_fast
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let plugin = format!(
        r#"[plugin]
id = "default"
description = "Gate invariant test plugin"

[[tools]]
id = "echo-tool"
description = "Echo helper command"
command = "echo"
args = ["ok"]

[gate]
ci_fast = [{gate_items}]
ci = []
flagship = []
"#
    );

    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        plugin,
    )
    .expect("write plugin.toml");

    std::fs::create_dir_all(repo.join(".agents/mcp/compas")).expect("mkdir compas dir");
    std::fs::write(
        repo.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 60
min_coverage_percent = 0.0
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

[exceptions]
max_exceptions = 10
max_suppressed_ratio = 0.30
max_exception_window_days = 90

[governance]
mandatory_checks = []
mandatory_failure_modes = []
min_failure_modes = 1

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
"#,
    )
    .expect("write quality_contract.toml");
}

fn write_plugin_with_missing_command(repo: &std::path::Path) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default"))
        .expect("mkdir plugin dir");
    std::fs::create_dir_all(repo.join(".agents/mcp/compas")).expect("mkdir compas dir");

    let plugin = r#"[plugin]
id = "default"
description = "Gate run_failed classification test plugin"

[tool_policy]
mode = "allowlist"
allow_commands = ["nonexistentcmd"]

[[tools]]
id = "broken-tool"
description = "Missing command for gate classification test"
command = "nonexistentcmd"
args = []

[gate]
ci_fast = ["broken-tool"]
ci = []
flagship = []
"#;

    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        plugin,
    )
    .expect("write plugin.toml");

    std::fs::write(
        repo.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999
"#,
    )
    .expect("write quality_contract.toml");
}

fn write_plugin_with_timeout(repo: &std::path::Path) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default"))
        .expect("mkdir plugin dir");
    std::fs::create_dir_all(repo.join(".agents/mcp/compas")).expect("mkdir compas dir");

    let plugin = r#"[plugin]
id = "default"
description = "Gate timeout classification test plugin"

[[tools]]
id = "slow-tool"
description = "Timeout tool for retryable classification"
command = "python3"
args = ["-c", "import time; time.sleep(0.2)"]
timeout_ms = 1
max_stdout_bytes = 1000
max_stderr_bytes = 1000

[gate]
ci_fast = ["slow-tool"]
ci = []
flagship = []
"#;

    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        plugin,
    )
    .expect("write plugin.toml");

    std::fs::write(
        repo.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 60
min_coverage_percent = 0.0
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

[exceptions]
max_exceptions = 10
max_suppressed_ratio = 0.30
max_exception_window_days = 90

[receipt_defaults]
min_duration_ms = 500
min_stdout_bytes = 10

[governance]
mandatory_checks = []
mandatory_failure_modes = []
min_failure_modes = 1

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
"#,
    )
    .expect("write quality_contract.toml");
}

#[tokio::test]
async fn gate_empty_sequence_fails_closed() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin(dir.path(), &[]);

    let out = gate(&dir.path().to_string_lossy(), GateKind::CiFast, true, false).await;
    assert!(!out.ok);
    assert_eq!(
        out.error.as_ref().map(|e| e.code.as_str()),
        Some("gate.empty_sequence")
    );
}

#[tokio::test]
async fn gate_duplicate_tools_fail_closed() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin(dir.path(), &["echo-tool", "echo-tool"]);

    let out = gate(&dir.path().to_string_lossy(), GateKind::CiFast, true, false).await;
    assert!(!out.ok);
    assert_eq!(
        out.error.as_ref().map(|e| e.code.as_str()),
        Some("gate.duplicate_tool_id")
    );
}

#[tokio::test]
async fn gate_missing_command_is_blocked_not_retryable() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin_with_missing_command(dir.path());

    let out = gate(
        &dir.path().to_string_lossy(),
        GateKind::CiFast,
        false,
        false,
    )
    .await;
    assert!(!out.ok);
    assert_eq!(
        out.error.as_ref().map(|e| e.code.as_str()),
        Some("gate.blocked"),
        "missing command must be treated as blocked non-transient failure"
    );
    let verdict = out.verdict.expect("verdict");
    assert!(matches!(
        verdict.decision.status,
        ai_dx_mcp::api::DecisionStatus::Blocked
    ));
    assert!(
        verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.run_failed"),
        "non-transient spawn error must map to gate.run_failed"
    );
}

#[tokio::test]
async fn gate_timeout_is_retryable_even_with_receipt_defaults() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin_with_timeout(dir.path());

    let out = gate(
        &dir.path().to_string_lossy(),
        GateKind::CiFast,
        false,
        false,
    )
    .await;
    assert!(!out.ok);
    assert_eq!(
        out.error.as_ref().map(|e| e.code.as_str()),
        Some("gate.retryable"),
        "timeout must remain retryable and not be masked by receipt contract defaults"
    );
    let verdict = out.verdict.expect("verdict");
    assert!(matches!(
        verdict.decision.status,
        ai_dx_mcp::api::DecisionStatus::Retryable
    ));
    assert!(
        !verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.receipt_contract_violated"),
        "receipt contract must not run when tool execution itself failed"
    );
}
