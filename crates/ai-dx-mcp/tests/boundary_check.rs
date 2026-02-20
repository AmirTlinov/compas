use ai_dx_mcp::{
    checks::boundary::run_boundary_check,
    config::{BoundaryCheckConfigV2, BoundaryRuleConfigV2},
};
use tempfile::tempdir;

fn cfg(regex: &str) -> BoundaryCheckConfigV2 {
    BoundaryCheckConfigV2 {
        id: "boundary".to_string(),
        include_globs: vec!["crates/**/*.rs".to_string()],
        exclude_globs: vec![],
        strip_rust_cfg_test_blocks: false,
        rules: vec![BoundaryRuleConfigV2 {
            id: "rule-1".to_string(),
            message: Some("no glob imports".to_string()),
            deny_regex: regex.to_string(),
        }],
    }
}

#[test]
fn boundary_detects_glob_import() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    std::fs::create_dir_all(repo.join("crates/x")).unwrap();
    std::fs::write(
        repo.join("crates/x/lib.rs"),
        "use crate::foo::*;\nfn x() {}\n",
    )
    .unwrap();

    let result = run_boundary_check(repo, &cfg(r"\buse\s+[^;]*::\*\s*;")).unwrap();
    assert_eq!(result.rules_checked, 1);
    assert_eq!(result.files_scanned, 1);
    assert!(
        result
            .violations
            .iter()
            .any(|v| v.code == "boundary.rule_violation")
    );
}

#[test]
fn boundary_invalid_regex_fails_closed() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    std::fs::create_dir_all(repo.join("crates/x")).unwrap();
    std::fs::write(repo.join("crates/x/lib.rs"), "fn x() {}\n").unwrap();

    let err = run_boundary_check(repo, &cfg("(")).unwrap_err();
    assert!(err.contains("failed to compile boundary rule regex"));
}

#[test]
fn boundary_strip_cfg_test_blocks_ignores_test_module_only_matches() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    std::fs::create_dir_all(repo.join("crates/x")).unwrap();
    std::fs::write(
        repo.join("crates/x/lib.rs"),
        r#"
fn runtime() -> usize {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        let _ = Some(1).unwrap();
    }
}
"#,
    )
    .unwrap();

    let mut cfg = cfg(r"\.unwrap\s*\(");
    cfg.strip_rust_cfg_test_blocks = true;
    let result = run_boundary_check(repo, &cfg).unwrap();
    assert!(result.violations.is_empty(), "{:?}", result.violations);
}

#[test]
fn boundary_strip_cfg_test_blocks_keeps_runtime_matches() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    std::fs::create_dir_all(repo.join("crates/x")).unwrap();
    std::fs::write(
        repo.join("crates/x/lib.rs"),
        r#"
fn runtime() {
    let _ = Some(1).unwrap();
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        let _ = Some(2).unwrap();
    }
}
"#,
    )
    .unwrap();

    let mut cfg = cfg(r"\.unwrap\s*\(");
    cfg.strip_rust_cfg_test_blocks = true;
    let result = run_boundary_check(repo, &cfg).unwrap();
    assert_eq!(result.violations.len(), 1, "{:?}", result.violations);
}
