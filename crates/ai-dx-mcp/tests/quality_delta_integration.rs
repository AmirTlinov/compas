use ai_dx_mcp::{
    api::{DecisionStatus, ValidateMode},
    app::validate,
};

fn write_repo(repo: &std::path::Path, max_loc: usize) {
    std::fs::create_dir_all(repo.join(".agents/mcp/compas/plugins/default"))
        .expect("mkdir plugin dir");
    std::fs::create_dir_all(repo.join(".agents/mcp/compas")).expect("mkdir compas dir");
    std::fs::create_dir_all(repo.join("src")).expect("mkdir src dir");

    std::fs::write(
        repo.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        format!(
            r#"[plugin]
id = "default"
description = "quality_delta integration test plugin"

[[tools]]
id = "noop"
description = "No-op gate tool for integration regression checks"
command = "echo"
args = ["ok"]

[gate]
ci_fast = ["noop"]
ci = []
flagship = []

[[checks.loc]]
id = "loc-main"
max_loc = {max_loc}
include_globs = ["src/**/*.rs"]
exclude_globs = []
baseline_path = ".agents/mcp/compas/baselines/loc.json"
"#
        ),
    )
    .expect("write plugin.toml");

    std::fs::write(
        repo.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0
"#,
    )
    .expect("write quality_contract.toml");

    // 3 non-empty lines.
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    )
    .expect("write src/lib.rs");
}

#[test]
fn quality_delta_blocks_trust_regression_after_baseline() {
    let dir = tempfile::tempdir().expect("temp repo");
    let repo_root = dir.path();
    let repo_root_str = repo_root.to_string_lossy().to_string();

    // Phase A: establish baseline from a clean posture.
    write_repo(repo_root, 100);
    let baseline = validate(&repo_root_str, ValidateMode::Warn, true, None);
    assert!(
        baseline.ok,
        "baseline validate should pass: {:?}",
        baseline.error
    );
    assert!(
        repo_root
            .join(".agents/mcp/compas/baselines/quality_snapshot.json")
            .is_file(),
        "quality snapshot must be written"
    );

    // Control run in ratchet should pass with same configuration.
    let control = validate(&repo_root_str, ValidateMode::Ratchet, false, None);
    assert!(
        control.ok,
        "control ratchet validate should pass before regression"
    );

    // Phase B: introduce regression by tightening loc threshold.
    write_repo(repo_root, 1);
    let out = validate(&repo_root_str, ValidateMode::Ratchet, false, None);

    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "quality_delta.trust_regression"),
        "expected quality_delta.trust_regression, got: {:?}",
        out.violations
    );
    assert!(
        out.findings_v2
            .iter()
            .any(|f| f.code == "finding.quality_delta.trust_regression"),
        "phase2 violation must be reflected in findings_v2"
    );
    let risk = out.risk_summary.as_ref().expect("risk summary");
    assert!(
        risk.by_severity.contains_key("critical"),
        "quality_delta.* findings are critical and must affect risk summary"
    );
    let trust = out.trust_score.as_ref().expect("trust score");
    assert!(
        trust.score < 100,
        "display trust must include phase2 violations and drop below 100"
    );
    let verdict = out.verdict.expect("verdict");
    assert_eq!(verdict.decision.status, DecisionStatus::Blocked);
    assert!(
        !out.ok,
        "ratchet validate must be blocked on trust regression"
    );
}
