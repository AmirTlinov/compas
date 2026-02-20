use ai_dx_mcp::{checks::duplicates::run_duplicates_check, config::DuplicatesCheckConfigV2};
use std::path::Path;
use tempfile::tempdir;

fn cfg() -> DuplicatesCheckConfigV2 {
    DuplicatesCheckConfigV2 {
        id: "dup".to_string(),
        include_globs: vec!["crates/**/*.txt".to_string()],
        exclude_globs: vec![],
        max_file_bytes: 4096,
        allowlist_globs: vec![],
        baseline_path: ".agents/mcp/compas/baselines/duplicates.json".to_string(),
    }
}

fn seed(repo: &Path, files: &[(&str, &str)]) {
    for (rel, body) in files {
        let full = repo.join(Path::new(rel));
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, body.as_bytes()).unwrap();
    }
}

#[test]
fn duplicates_reports_observation_when_found() {
    let dir = tempdir().unwrap();
    seed(
        dir.path(),
        &[("crates/x/a.txt", "same"), ("crates/x/b.txt", "same")],
    );
    let r = run_duplicates_check(dir.path(), &cfg()).unwrap();
    assert!(r.violations.iter().any(|v| v.code == "duplicates.found"));
    assert!(r.violations.iter().all(|v| {
        v.code != "duplicates.found" || matches!(v.tier, ai_dx_mcp::api::ViolationTier::Observation)
    }));
    assert!(r.groups_total >= 1);
}

#[test]
fn duplicates_allowlist_is_group_scoped_all_paths_must_match() {
    let dir = tempdir().unwrap();
    seed(
        dir.path(),
        &[("crates/x/a.txt", "same"), ("crates/x/b.txt", "same")],
    );

    let mut c = cfg();
    c.allowlist_globs = vec!["crates/x/a.txt".to_string()];
    let r = run_duplicates_check(dir.path(), &c).unwrap();
    assert!(r.violations.iter().any(|v| v.code == "duplicates.found"));

    c.allowlist_globs = vec!["crates/x/*.txt".to_string()];
    let r = run_duplicates_check(dir.path(), &c).unwrap();
    assert!(r.violations.is_empty());
}
