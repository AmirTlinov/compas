use ai_dx_mcp::{
    api::{DecisionStatus, ValidateMode},
    app::validate,
    repo::{RepoConfigError, load_repo_config},
};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write fixture");
}

#[test]
fn duplicate_check_id_across_plugins_fails_closed() {
    let dir = tempdir().expect("tempdir");

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin used for duplicate-check anti-gaming coverage"

[[checks.boundary]]
id = "boundary-main"
include_globs = ["crates/**/*.rs"]
exclude_globs = [".git/**"]
strip_rust_cfg_test_blocks = true

[[checks.boundary.rules]]
id = "no-glob-reexports"
deny_regex = "\\bpub\\s+use\\s+[^;]*::\\*\\s*;"
"#,
    );

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/p04/plugin.toml"),
        r#"
[plugin]
id = "p04"
description = "P04 duplicate-check anti-gaming regression plugin fixture"

[[checks.boundary]]
id = "boundary-main"
include_globs = ["crates/**/*.rs"]
exclude_globs = [".git/**"]
strip_rust_cfg_test_blocks = true

[[checks.boundary.rules]]
id = "no-runtime-stdout"
deny_regex = "\\beprintln!\\s*\\("
"#,
    );

    let err =
        load_repo_config(dir.path()).expect_err("must fail on duplicate check id across plugins");
    match err {
        RepoConfigError::DuplicateCheckId {
            kind,
            check_id,
            plugin_id,
            previous_plugin_id,
        } => {
            assert_eq!(kind, "boundary");
            assert_eq!(check_id, "boundary-main");
            assert_eq!(plugin_id, "p04");
            assert_eq!(previous_plugin_id, "default");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn mandatory_check_removed_is_blocked_in_validate() {
    let dir = tempdir().expect("tempdir");

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin without boundary checks for anti-gaming test"

[[checks.loc]]
id = "loc-main"
max_loc = 100
include_globs = ["src/**/*.rs"]
exclude_globs = []
baseline_path = ".agents/mcp/compas/baselines/loc.json"
"#,
    );

    write(
        &dir.path().join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0

[governance]
mandatory_checks = ["boundary", "loc"]
mandatory_failure_modes = []
min_failure_modes = 1
"#,
    );

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Ratchet,
        false,
        None,
    );
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "config.mandatory_check_removed"),
        "expected config.mandatory_check_removed for missing mandatory check"
    );
    let verdict = out.verdict.expect("verdict should exist");
    assert_eq!(verdict.decision.status, DecisionStatus::Blocked);
    assert!(
        out.ok == false,
        "runtime validation must fail when mandatory checks are removed"
    );
}

#[test]
fn config_threshold_weakened_is_blocked_in_validate() {
    let dir = tempdir().expect("tempdir");

    write(
        &dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Default plugin for config hash anti-gaming fixture"

[[checks.loc]]
id = "loc-main"
max_loc = 120
include_globs = ["src/**/*.rs"]
exclude_globs = [".git/**"]
baseline_path = ".agents/mcp/compas/baselines/loc.json"
"#,
    );

    // Keep a source file present so the LOC check has deterministic scan behavior.
    write(&dir.path().join("src/lib.rs"), "pub fn main() {}\n");

    write(
        &dir.path().join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0

[governance]
mandatory_checks = ["loc"]
mandatory_failure_modes = []
min_failure_modes = 1
config_hash = "sha256:tampered"
"#,
    );

    let out = validate(
        &dir.path().to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );

    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "config.threshold_weakened"),
        "expected config.threshold_weakened when governance hash does not match"
    );
    let out_warn = out;
    assert!(
        out_warn
            .violations
            .iter()
            .any(|v| v.code == "config.threshold_weakened"),
        "warn mode should still expose config threshold drift"
    );
}
