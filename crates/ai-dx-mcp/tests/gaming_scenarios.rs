//! Gaming scenario tests: verify anti-gaming mechanisms in quality_delta.

use ai_dx_mcp::checks::quality_delta::{FileUniverse, QualitySnapshot, compare};
use ai_dx_mcp::config::QualityContractConfig;

fn default_contract() -> QualityContractConfig {
    toml::from_str("").unwrap()
}

fn baseline() -> QualitySnapshot {
    QualitySnapshot {
        version: 1,
        trust_score: 85,
        coverage_covered: 8,
        coverage_total: 10,
        weighted_risk: 12,
        findings_total: 3,
        risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)]
            .into_iter()
            .collect(),
        loc_per_file: [
            ("src/main.rs".to_string(), 100),
            ("src/lib.rs".to_string(), 50),
        ]
        .into_iter()
        .collect(),
        surface_items: vec!["src/api.rs::pub_fn:validate".to_string()],
        duplicate_groups: vec![],
        file_universe: FileUniverse {
            loc_universe: 50,
            loc_scanned: 45,
            surface_universe: 50,
            surface_scanned: 45,
            boundary_universe: 50,
            boundary_scanned: 50,
            duplicates_universe: 50,
            duplicates_scanned: 50,
        },
        written_at: "2026-02-17T12:00:00Z".to_string(),
        written_by: None,
        config_hash: "sha256:abc".to_string(),
    }
}

#[test]
fn gaming_severity_shift_fix_lows_introduce_high() {
    let b = baseline();
    let mut c = baseline();
    c.weighted_risk = b.weighted_risk + 5;
    c.trust_score = b.trust_score - 3;
    let violations = compare(&b, &c, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.trust_regression")
    );
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.risk_profile_regression")
    );
}

#[test]
fn gaming_coverage_stripping() {
    let b = baseline();
    let mut c = baseline();
    c.coverage_covered = 5;
    let violations = compare(&b, &c, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.coverage_regression")
    );
}

#[test]
fn gaming_scope_narrowing_via_excludes() {
    let b = baseline();
    let mut c = baseline();
    c.file_universe.loc_scanned = 10;
    let violations = compare(&b, &c, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.scope_narrowed")
    );
}

#[test]
fn gaming_config_hash_tampering() {
    let b = baseline();
    let mut c = baseline();
    c.config_hash = "sha256:weakened_thresholds".to_string();
    let violations = compare(&b, &c, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.config_changed")
    );
}
