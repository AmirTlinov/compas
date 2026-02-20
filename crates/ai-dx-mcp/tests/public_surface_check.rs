use ai_dx_mcp::{
    checks::surface::run_surface_check,
    config::{SurfaceCheckConfigV2, SurfaceRuleConfigV2},
};
use tempfile::tempdir;

fn cfg(max_items: usize) -> SurfaceCheckConfigV2 {
    SurfaceCheckConfigV2 {
        id: "surface".to_string(),
        max_items,
        include_globs: vec!["crates/**/*.rs".to_string()],
        exclude_globs: vec![],
        rules: vec![SurfaceRuleConfigV2 {
            file_globs: vec![],
            regex: "^pub\\s+fn\\s+([A-Za-z0-9_]+)".to_string(),
            description: Some("fn".to_string()),
        }],
        baseline_path: ".agents/mcp/compas/baselines/public_surface.json".to_string(),
    }
}

fn seed_repo(repo: &std::path::Path, body: &str) {
    std::fs::create_dir_all(repo.join("crates/x")).unwrap();
    std::fs::write(repo.join("crates/x/lib.rs"), body).unwrap();
}

#[test]
fn public_surface_scan_collects_items() {
    let dir = tempdir().unwrap();
    seed_repo(dir.path(), "pub fn a() {}\npub fn b() {}\n");
    let out = run_surface_check(dir.path(), &cfg(10)).unwrap();
    assert_eq!(out.items_total, 2);
    assert_eq!(out.current_items.len(), 2);
    assert!(out.violations.is_empty());
}

#[test]
fn public_surface_max_is_observation() {
    let dir = tempdir().unwrap();
    seed_repo(dir.path(), "pub fn a() {}\npub fn b() {}\n");
    let out = run_surface_check(dir.path(), &cfg(1)).unwrap();
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "surface.max_exceeded")
    );
    assert!(
        out.violations
            .iter()
            .all(|v| { matches!(v.tier, ai_dx_mcp::api::ViolationTier::Observation) })
    );
}
