use ai_dx_mcp::api::{DecisionStatus, ValidateMode};

#[test]
fn validate_warn_returns_verdict_pass() {
    let repo_root = {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    };
    let output = ai_dx_mcp::app::validate(
        &repo_root.to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );
    assert_eq!(output.schema_version, "3");
    let verdict = output.verdict.expect("verdict must be present");
    assert!(matches!(
        verdict.decision.status,
        DecisionStatus::Pass | DecisionStatus::Blocked | DecisionStatus::Retryable
    ));
    assert!(output.ok);
}
