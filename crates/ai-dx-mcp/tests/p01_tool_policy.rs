use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Report {
    status: String,
    #[serde(default)]
    findings: Vec<serde_json::Value>,
    #[serde(default)]
    errors: Vec<String>,
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

fn script_path() -> PathBuf {
    workspace_root().join("tools/custom/p01-shell-policy/check_tool_policy.py")
}

fn write(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, content).expect("write fixture");
}

fn run_script(repo_root: &std::path::Path) -> (i32, Report) {
    let output = std::process::Command::new("python3")
        .arg(script_path())
        .arg("--repo-root")
        .arg(repo_root)
        .output()
        .expect("run checker");
    let code = output.status.code().unwrap_or(-1);
    let report: Report =
        serde_json::from_slice(&output.stdout).expect("checker must return JSON report");
    assert!(output.stderr.is_empty(), "checker stderr must be empty");
    (code, report)
}

#[test]
fn p01_tool_policy_passes_when_no_shell_commands_present() {
    let dir = tempfile::tempdir().expect("tmp");

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Safe fixture plugin"

[[tools]]
id = "safe-cargo"
description = "Safe command"
command = "cargo"
"#,
    );
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/inline/plugin.toml"),
        r#"
[plugin]
id = "inline"
description = "Inline command fixture"
tool_import_globs = ["tools/custom/safe/tool.toml"]

[[tools]]
id = "inline"
description = "Another safe command"
command = "python"
"#,
    );
    write(
        &dir.path().join("tools/custom/safe/tool.toml"),
        r#"
[tool]
id = "safe-tool"
description = "Fixture imported tool"
command = "echo"
"#,
    );

    let (code, report) = run_script(dir.path());
    assert_eq!(code, 0, "expected clean exit on pass");
    assert_eq!(report.status, "pass");
    assert!(report.findings.is_empty());
    assert!(report.errors.is_empty());
}

#[test]
fn p01_tool_policy_fails_on_banned_shell_commands() {
    let dir = tempfile::tempdir().expect("tmp");

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Unsafe fixture plugin"

[[tools]]
id = "shell-tool"
description = "Banned shell command"
command = "bash"
"#,
    );
    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/imported/plugin.toml"),
        r#"
[plugin]
id = "imported"
description = "Imported fixture plugin"
tool_import_globs = ["tools/custom/unsafe/p01-policy.toml"]
"#,
    );
    write(
        &dir.path().join("tools/custom/unsafe/p01-policy.toml"),
        r#"
[tool]
id = "unsafe-tool"
description = "Imported unsafe command"
command = "pwsh"
"#,
    );

    let (code, report) = run_script(dir.path());
    assert_eq!(code, 2, "expected fail exit code for forbidden command");
    assert_eq!(report.status, "fail");
    assert!(report.findings.len() >= 2);
    let statuses: Vec<_> = report
        .findings
        .iter()
        .filter_map(|finding| finding.get("code"))
        .filter_map(|v| v.as_str())
        .collect();
    assert!(statuses.contains(&"tool_policy.forbidden_shell"));
}
