use ai_dx_mcp::{
    api::EffectiveConfigSource,
    checks::env_registry::run_env_registry_check,
    config::{EnvRegistryCheckConfigV2, ProjectTool},
};
use std::collections::BTreeMap;
use tempfile::tempdir;

fn cfg() -> EnvRegistryCheckConfigV2 {
    EnvRegistryCheckConfigV2 {
        id: "env".to_string(),
        registry_path: ".agents/mcp/compas/env_registry.toml".to_string(),
    }
}

fn tool_with_env(id: &str, env_name: &str, env_value: &str) -> ProjectTool {
    let mut env = BTreeMap::new();
    env.insert(env_name.to_string(), env_value.to_string());

    ProjectTool {
        id: id.to_string(),
        description: format!("Fixture tool {id}"),
        command: "echo".to_string(),
        args: vec![],
        cwd: None,
        timeout_ms: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
        receipt_contract: None,
        env,
    }
}

#[test]
fn missing_registry_produces_violation() {
    let dir = tempdir().unwrap();
    let mut tools = BTreeMap::new();
    tools.insert(
        "t1".to_string(),
        tool_with_env("t1", "CARGO_TERM_COLOR", "always"),
    );

    let r = run_env_registry_check(dir.path(), &cfg(), &tools);
    assert!(
        r.violations
            .iter()
            .any(|v| v.code == "env_registry.registry_missing")
    );
    assert_eq!(r.summary.used_vars, vec!["CARGO_TERM_COLOR".to_string()]);
}

#[test]
fn unregistered_tool_env_var_produces_violation() {
    let dir = tempdir().unwrap();
    let registry_path = dir.path().join(".agents/mcp/compas");
    std::fs::create_dir_all(&registry_path).unwrap();
    std::fs::write(
        registry_path.join("env_registry.toml"),
        r#"
[[vars]]
name = "REGISTERED_ONLY"
required = false
"#,
    )
    .unwrap();

    let mut tools = BTreeMap::new();
    tools.insert(
        "t1".to_string(),
        tool_with_env("t1", "UNREGISTERED_VAR", "x"),
    );

    let r = run_env_registry_check(dir.path(), &cfg(), &tools);
    assert!(
        r.violations
            .iter()
            .any(|v| v.code == "env_registry.unregistered_usage")
    );
}

#[test]
fn required_missing_without_default_is_violation() {
    let dir = tempdir().unwrap();
    let registry_path = dir.path().join(".agents/mcp/compas");
    std::fs::create_dir_all(&registry_path).unwrap();
    std::fs::write(
        registry_path.join("env_registry.toml"),
        r#"
[[vars]]
name = "REQ_VAR"
required = true
"#,
    )
    .unwrap();

    let tools = BTreeMap::new();
    let r = run_env_registry_check(dir.path(), &cfg(), &tools);

    assert!(
        r.violations
            .iter()
            .any(|v| v.code == "env_registry.required_missing")
    );

    let entry = r
        .summary
        .entries
        .iter()
        .find(|e| e.name == "REQ_VAR")
        .unwrap();
    assert!(matches!(entry.source, EffectiveConfigSource::Unset));
}

#[test]
fn sensitive_default_value_is_redacted() {
    let dir = tempdir().unwrap();
    let registry_path = dir.path().join(".agents/mcp/compas");
    std::fs::create_dir_all(&registry_path).unwrap();
    std::fs::write(
        registry_path.join("env_registry.toml"),
        r#"
[[vars]]
name = "TOKEN_VAR"
required = false
sensitive = true
default = "super-secret"
"#,
    )
    .unwrap();

    let tools = BTreeMap::new();
    let r = run_env_registry_check(dir.path(), &cfg(), &tools);

    let entry = r
        .summary
        .entries
        .iter()
        .find(|e| e.name == "TOKEN_VAR")
        .unwrap();
    assert!(matches!(entry.source, EffectiveConfigSource::Default));
    assert_eq!(entry.value.as_deref(), Some("<redacted>"));
}
