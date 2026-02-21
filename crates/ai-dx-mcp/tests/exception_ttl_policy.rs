use ai_dx_mcp::{api::ValidateMode, app::validate};
use std::path::Path;

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

#[test]
fn validate_fails_closed_when_allowlist_expires_at_is_missing() {
    let dir = tempfile::tempdir().expect("temp repo");
    let repo_root = dir.path();

    write_file(
        &repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "Exception TTL policy e2e test"

[[checks.loc]]
id = "loc-main"
max_loc = 1
include_globs = ["src/**/*.rs"]
exclude_globs = []
baseline_path = ".agents/mcp/compas/baselines/loc.json"

[gate]
ci_fast = []
ci = []
flagship = []
"#,
    );

    write_file(
        &repo_root.join(".agents/mcp/compas/allowlist.toml"),
        r#"
[[exceptions]]
id = "ex-ttl-missing"
rule = "loc.max_exceeded"
path = "src/lib.rs"
owner = "team-qa"
reason = "temporary regression"
"#,
    );

    write_file(
        &repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999

[exceptions]
max_exceptions = 10
max_suppressed_ratio = 1.0
max_exception_window_days = 90

[governance]
mandatory_checks = []
mandatory_failure_modes = []
min_failure_modes = 1

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 1.0
"#,
    );

    write_file(
        &repo_root.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    );

    let output = validate(
        &repo_root.to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );
    assert!(
        output
            .violations
            .iter()
            .any(|v| v.code == "exception.allowlist_invalid"),
        "missing expires_at must produce exception.allowlist_invalid: {:?}",
        output.violations
    );
    assert!(
        output
            .violations
            .iter()
            .any(|v| v.code == "loc.max_exceeded"),
        "original violation must remain visible when allowlist fails closed: {:?}",
        output.violations
    );
    assert!(
        output.suppressed.is_empty(),
        "missing expires_at must prevent suppression: {:?}",
        output.suppressed
    );
}
