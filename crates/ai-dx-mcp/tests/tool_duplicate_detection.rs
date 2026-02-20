use ai_dx_mcp::{api::ValidateMode, app::validate};

fn write_base(repo: &std::path::Path, plugin_body: &str) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default")).expect("mkdir plugin");
    std::fs::create_dir_all(repo.join(".agents/mcp/compas")).expect("mkdir compas");
    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        plugin_body,
    )
    .expect("write plugin");
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
min_duration_ms = 0
min_stdout_bytes = 0

[governance]
mandatory_checks = []
mandatory_failure_modes = []
min_failure_modes = 1

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
"#,
    )
    .expect("write quality_contract");
}

#[test]
fn duplicate_exact_ignores_gate_membership() {
    let dir = tempfile::tempdir().expect("tmp");
    let plugin = r#"
[plugin]
id = "default"
description = "Duplicate exact test"
tool_import_globs = []

[[tools]]
id = "t-fast"
description = "Run cargo test fast"
command = "cargo"
args = ["test"]

[[tools]]
id = "t-ci"
description = "Run cargo test ci"
command = "cargo"
args = ["test"]

[gate]
ci_fast = ["t-fast"]
ci = ["t-ci"]
flagship = ["t-ci"]
"#;
    write_base(dir.path(), plugin);

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Ratchet,
        false,
        None,
    );
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "tools.duplicate_exact"),
        "same execution signature must be flagged even if gate membership differs"
    );
}

#[test]
fn duplicate_exact_does_not_trigger_for_distinct_timeout_contract() {
    let dir = tempfile::tempdir().expect("tmp");
    let plugin = r#"
[plugin]
id = "default"
description = "Duplicate exact timeout differentiation test"
tool_import_globs = []

[[tools]]
id = "t-short"
description = "Run cargo test short timeout"
command = "cargo"
args = ["test"]
timeout_ms = 1000

[[tools]]
id = "t-long"
description = "Run cargo test long timeout"
command = "cargo"
args = ["test"]
timeout_ms = 5000

[gate]
ci_fast = ["t-short", "t-long"]
ci = []
flagship = []
"#;
    write_base(dir.path(), plugin);

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Ratchet,
        false,
        None,
    );
    assert!(
        !out.violations
            .iter()
            .any(|v| v.code == "tools.duplicate_exact"),
        "tools with distinct runtime contract must not be marked exact duplicates"
    );
}
