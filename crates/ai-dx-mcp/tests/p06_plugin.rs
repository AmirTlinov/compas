use ai_dx_mcp::repo::{load_repo_config, RepoConfigError};
use std::fs;
use std::path::Path;

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, body).expect("write file");
}

#[test]
fn p06_plugin_loads_with_unique_ids() {
    let repo = tempfile::tempdir().expect("tmp repo");
    write(
        &repo.path().join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Test default plugin"

[[checks.loc]]
id = "default-loc"
max_loc = 700
include_globs = ["crates/**/*.rs"]
exclude_globs = ["**/target/**"]
baseline_path = "loc.json"

[[checks.complexity_budget]]
id = "default-complexity"
include_globs = ["crates/ai-dx-mcp/src/**/*.rs"]
exclude_globs = ["**/target/**"]
max_function_lines = 180
max_cyclomatic = 60
max_cognitive = 140

[[checks.tool_budget]]
id = "default-tool-budget"
max_tools_total = 2
max_tools_per_plugin = 2
max_gate_tools_per_kind = 2
max_checks_total = 10
"#,
    );

    write(
        &repo.path().join(".agents/mcp/compas/plugins/p06/plugin.toml"),
        r#"
[plugin]
id = "p06"
description = "Test p06 plugin"

tool_import_globs = []

[[checks.loc]]
id = "p06-loc"
max_loc = 500
include_globs = ["crates/ai-dx-mcp/src/**/*.rs"]
exclude_globs = ["**/target/**", "**/tests/**"]
baseline_path = "loc.json"

[[checks.complexity_budget]]
id = "p06-complexity"
include_globs = ["crates/ai-dx-mcp/src/**/*.rs"]
exclude_globs = ["**/target/**"]
max_function_lines = 180
max_cyclomatic = 60
max_cognitive = 140

[[checks.tool_budget]]
id = "p06-tool-budget"
max_tools_total = 2
max_tools_per_plugin = 2
max_gate_tools_per_kind = 2
max_checks_total = 20
"#,
    );

    let cfg = load_repo_config(repo.path()).expect("load repo config with p06");
    let loc_ids: Vec<_> = cfg.checks.loc.iter().map(|c| c.id.as_str()).collect();
    assert!(loc_ids.contains(&"default-loc"));
    assert!(loc_ids.contains(&"p06-loc"));
    assert_eq!(cfg.checks.complexity_budget.len(), 2);
    assert_eq!(cfg.checks.tool_budget.len(), 2);

    let cx_ids: Vec<_> = cfg
        .checks
        .complexity_budget
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert!(cx_ids.contains(&"default-complexity"));
    assert!(cx_ids.contains(&"p06-complexity"));
}

#[test]
fn p06_check_id_collision_is_duplicate_check_error() {
    let repo = tempfile::tempdir().expect("tmp repo");
    write(
        &repo.path().join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Test default plugin"

[[checks.complexity_budget]]
id = "dup-complexity"
include_globs = ["crates/**/*.rs"]
exclude_globs = ["**/target/**"]
max_function_lines = 180
max_cyclomatic = 60
max_cognitive = 140
"#,
    );
    write(
        &repo.path().join(".agents/mcp/compas/plugins/p06/plugin.toml"),
        r#"
[plugin]
id = "p06"
description = "Test p06 plugin with duplicate id"

[[checks.complexity_budget]]
id = "dup-complexity"
include_globs = ["crates/**/*.rs"]
exclude_globs = ["**/target/**"]
max_function_lines = 180
max_cyclomatic = 60
max_cognitive = 140
"#,
    );

    let err = load_repo_config(repo.path()).expect_err("must fail on duplicate check id");
    match err {
        RepoConfigError::DuplicateCheckId {
            kind,
            check_id,
            plugin_id,
            previous_plugin_id,
            ..
        } => {
            assert_eq!(kind, "complexity_budget");
            assert_eq!(check_id, "dup-complexity");
            assert!(
                matches!(
                    (plugin_id.as_str(), previous_plugin_id.as_str()),
                    ("p06", "default") | ("default", "p06")
                ),
                "{plugin_id} -> {previous_plugin_id}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
