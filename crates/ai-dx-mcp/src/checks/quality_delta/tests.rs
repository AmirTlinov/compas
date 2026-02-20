use super::*;

fn default_contract() -> QualityContractConfig {
    toml::from_str("").unwrap()
}

fn sample_snapshot() -> QualitySnapshot {
    QualitySnapshot {
        version: SNAPSHOT_VERSION,
        trust_score: 85,
        coverage_covered: 8,
        coverage_total: 10,
        weighted_risk: 12,
        findings_total: 3,
        risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)]
            .into_iter()
            .collect(),
        loc_per_file: [("src/main.rs".to_string(), 100)].into_iter().collect(),
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
fn snapshot_roundtrip_deterministic() {
    let snap = sample_snapshot();
    let json1 = serde_json::to_string_pretty(&snap).unwrap();
    let parsed: QualitySnapshot = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string_pretty(&parsed).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn snapshot_version_checked() {
    let mut snap = sample_snapshot();
    snap.version = 999;
    let json = serde_json::to_string(&snap).unwrap();
    let err = load_snapshot_from_str(&json);
    assert!(err.is_err());
}

#[test]
fn trust_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.trust_score = baseline.trust_score - 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.trust_regression")
    );
}

#[test]
fn trust_below_minimum_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.trust_score = 30;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.trust_below_minimum")
    );
}

#[test]
fn coverage_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.coverage_covered = baseline.coverage_covered - 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.coverage_regression")
    );
}

#[test]
fn risk_profile_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.weighted_risk = baseline.weighted_risk + 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.risk_profile_regression")
    );
}

#[test]
fn loc_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.loc_per_file.insert("src/main.rs".to_string(), 200);
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.loc_regression")
    );
}

#[test]
fn surface_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current
        .surface_items
        .push("src/api.rs::pub_fn:new_thing".to_string());
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.surface_regression")
    );
}

#[test]
fn scope_narrowing_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.file_universe.loc_scanned = 20;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.scope_narrowed")
    );
}

#[test]
fn config_changed_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.config_hash = "sha256:different".to_string();
    let violations = compare(&baseline, &current, &default_contract());
    assert!(
        violations
            .iter()
            .any(|v| v.code == "quality_delta.config_changed")
    );
}

#[test]
fn no_regressions_yields_empty() {
    let baseline = sample_snapshot();
    let current = sample_snapshot();
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.is_empty(), "{violations:#?}");
}
