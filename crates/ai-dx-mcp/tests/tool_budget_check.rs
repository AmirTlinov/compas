use ai_dx_mcp::{api::ValidateMode, app::validate};

fn write_plugin(repo: &std::path::Path, body: &str) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default"))
        .expect("mkdir plugin dir");
    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        body,
    )
    .expect("write plugin.toml");
}

#[test]
fn tool_budget_detects_tool_count_regression() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin(
        dir.path(),
        r#"[plugin]
id = "default"
description = "Tool budget regression test plugin"

[[tools]]
id = "t1"
description = "tool 1 for budget test"
command = "echo"
args = ["ok"]

[[tools]]
id = "t2"
description = "tool 2 for budget test"
command = "echo"
args = ["ok"]

[[tools]]
id = "t3"
description = "tool 3 for budget test"
command = "echo"
args = ["ok"]

[gate]
ci_fast = ["t1"]
ci = []
flagship = []

[[checks.tool_budget]]
id = "tool-budget"
max_tools_total = 2
max_tools_per_plugin = 10
max_gate_tools_per_kind = 10
max_checks_total = 10
"#,
    );

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Strict,
        false,
        None,
    );
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "tool_budget.max_tools_total_exceeded"),
        "{:?}",
        out.violations
    );
}

#[test]
fn tool_budget_detects_checks_count_regression() {
    let dir = tempfile::tempdir().expect("temp repo");
    write_plugin(
        dir.path(),
        r#"[plugin]
id = "default"
description = "Check budget regression test plugin"

[[tools]]
id = "t1"
description = "tool 1 for budget test"
command = "echo"
args = ["ok"]

[gate]
ci_fast = ["t1"]
ci = []
flagship = []

[[checks.supply_chain]]
id = "supply-chain"

[[checks.tool_budget]]
id = "tool-budget"
max_tools_total = 10
max_tools_per_plugin = 10
max_gate_tools_per_kind = 10
max_checks_total = 1
"#,
    );

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Strict,
        false,
        None,
    );
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "tool_budget.max_checks_total_exceeded"),
        "{:?}",
        out.violations
    );
}
