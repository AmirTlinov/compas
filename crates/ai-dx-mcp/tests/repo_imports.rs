use ai_dx_mcp::repo::{RepoConfigError, load_repo_config};
use std::fs;
use std::path::Path;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write file");
}

#[test]
fn imports_tools_from_repo_glob() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin for import tests"
tool_import_globs = ["tools/custom/**/tool.toml"]

[gate]
ci_fast = ["cargo-test"]
"#,
    );
    write(
        &dir.path().join("tools/custom/cargo-test/tool.toml"),
        r#"
[tool]
id = "cargo-test"
description = "Run cargo test in fixture"
command = "cargo"
args = ["test"]

[tool.env]
CARGO_TERM_COLOR = "always"
"#,
    );

    let cfg = load_repo_config(dir.path()).expect("load repo config");
    let tool = cfg.tools.get("cargo-test").expect("imported tool exists");
    assert_eq!(tool.command, "cargo");
    assert_eq!(tool.args, vec!["test"]);
    assert_eq!(
        tool.env.get("CARGO_TERM_COLOR"),
        Some(&"always".to_string())
    );
    assert_eq!(cfg.gate.ci_fast, vec!["cargo-test"]);
}

#[test]
fn duplicate_inline_and_imported_tool_id_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin with duplicate tool ids"
tool_import_globs = ["tools/custom/**/tool.toml"]

[[tools]]
id = "cargo-test"
description = "Inline duplicate"
command = "cargo"
"#,
    );
    write(
        &dir.path().join("tools/custom/cargo-test/tool.toml"),
        r#"
[tool]
id = "cargo-test"
description = "Imported duplicate"
command = "cargo"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on duplicate tool id");
    match err {
        RepoConfigError::DuplicateTool { tool_id, .. } => assert_eq!(tool_id, "cargo-test"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn invalid_imported_tool_toml_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin with broken imported tool"
tool_import_globs = ["tools/custom/**/tool.toml"]
"#,
    );
    write(
        &dir.path().join("tools/custom/bad/tool.toml"),
        r#"
[tool]
id = "broken"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on broken tool config");
    match err {
        RepoConfigError::ParseImportedTool { .. } => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn missing_plugin_description_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
tool_import_globs = ["tools/custom/**/tool.toml"]
"#,
    );
    write(
        &dir.path().join("tools/custom/cargo-test/tool.toml"),
        r#"
[tool]
id = "cargo-test"
description = "Run cargo test in fixture"
command = "cargo"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on missing plugin description");
    match err {
        RepoConfigError::ParsePlugin { .. } => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn unknown_gate_tool_reference_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin for unknown gate tool test"

[gate]
ci_fast = ["missing-tool"]
"#,
    );
    let err = load_repo_config(dir.path()).expect_err("must fail on unknown gate tool");
    match err {
        RepoConfigError::UnknownGateTool { tool_id, .. } => assert_eq!(tool_id, "missing-tool"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn unknown_plugin_field_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin with unknown field"
unexpected_key = "boom"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on unknown plugin field");
    match err {
        RepoConfigError::ParsePlugin { .. } => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn unknown_tool_field_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin for unknown tool field test"
tool_import_globs = ["tools/custom/**/tool.toml"]
"#,
    );
    write(
        &dir.path().join("tools/custom/cargo-test/tool.toml"),
        r#"
[tool]
id = "cargo-test"
description = "Tool with unknown field"
command = "cargo"
unexpected_key = "boom"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on unknown tool field");
    match err {
        RepoConfigError::ParseImportedTool { .. } => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn tool_command_policy_rejects_unknown_command_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Plugin for command policy default deny test"

[[tools]]
id = "custom-tool"
description = "Custom command tool for deny test"
command = "totally-custom-cli"
"#,
    );

    let err = load_repo_config(dir.path()).expect_err("must fail on unknown command");
    match err {
        RepoConfigError::ToolCommandPolicyViolation {
            plugin_id,
            tool_id,
            command,
            ..
        } => {
            assert_eq!(plugin_id, "default");
            assert_eq!(tool_id, "custom-tool");
            assert_eq!(command, "totally-custom-cli");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn tool_command_policy_allow_any_allows_custom_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Plugin for allow_any command policy test"

[tool_policy]
mode = "allow_any"

[[tools]]
id = "custom-tool"
description = "Custom command tool for allow_any test"
command = "totally-custom-cli"
"#,
    );

    let cfg = load_repo_config(dir.path()).expect("allow_any should pass");
    assert!(cfg.tools.contains_key("custom-tool"));
}

#[test]
fn tool_command_policy_allowlist_accepts_custom_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Plugin for allowlist override command policy test"

[tool_policy]
mode = "allowlist"
allow_commands = ["totally-custom-cli"]

[[tools]]
id = "custom-tool"
description = "Custom command tool for allowlist override test"
command = "totally-custom-cli"
"#,
    );

    let cfg = load_repo_config(dir.path()).expect("allowlist override should pass");
    assert!(cfg.tools.contains_key("custom-tool"));
}
