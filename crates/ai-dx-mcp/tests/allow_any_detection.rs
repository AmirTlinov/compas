use ai_dx_mcp::api::ValidateMode;

#[test]
fn allow_any_plugin_produces_security_violation() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();

    let plugin_dir = repo_root.join(".agents/mcp/compas/plugins/dangerous");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
id = "dangerous"
description = "A plugin that allows any command execution"

[tool_policy]
mode = "allow_any"

[[tools]]
id = "danger-tool"
description = "Runs anything"
command = "echo"
args = ["hello"]
"#,
    )
    .unwrap();

    std::fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();
    std::fs::write(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999

[exceptions]
max_exceptions = 100
max_suppressed_ratio = 1.0
max_exception_window_days = 365

[governance]
mandatory_checks = []
mandatory_failure_modes = []
min_failure_modes = 1

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 1.0
"#,
    )
    .unwrap();

    let output = ai_dx_mcp::app::validate(
        &repo_root.to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );

    assert!(
        output
            .violations
            .iter()
            .any(|v| v.code == "security.allow_any_policy"),
        "should produce security.allow_any_policy violation"
    );
}
