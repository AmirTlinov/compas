use super::{
    check_receipt_contract, classify_run_failed, effective_receipt_contract, gate_fail,
    required_tools_for_changes, unmapped_path_violations,
};
use crate::{
    api::{
        ApiError, DecisionStatus, GateKind, Receipt, ValidateMode, ValidateOutput, ViolationTier,
    },
    config::{ImpactRule, ImpactUnmappedPathPolicy, QualityContractConfig, ToolReceiptContract},
};

fn mk_receipt(stdout_tail: &str, stderr_tail: &str) -> Receipt {
    Receipt {
        tool_id: "tool-x".to_string(),
        success: true,
        exit_code: Some(0),
        timed_out: false,
        duration_ms: 1_500,
        command: "cmd".to_string(),
        args: vec![],
        stdout_tail: stdout_tail.to_string(),
        stderr_tail: stderr_tail.to_string(),
        stdout_bytes: 128,
        stderr_bytes: 64,
        stdout_sha256: "a".repeat(64),
        stderr_sha256: "b".repeat(64),
        structured_report: None,
    }
}

fn mk_validate_output(ok: bool) -> ValidateOutput {
    ValidateOutput {
        ok,
        error: None,
        schema_version: "3".to_string(),
        repo_root: ".".to_string(),
        mode: ValidateMode::Ratchet,
        violations: vec![],
        findings_v2: vec![],
        suppressed: vec![],
        loc: None,
        boundary: None,
        public_surface: None,
        effective_config: None,
        risk_summary: None,
        coverage: None,
        trust_score: None,
        verdict: None,
        quality_posture: None,
        agent_digest: None,
        summary_md: None,
        payload_meta: None,
    }
}

#[test]
fn gate_fail_validate_failed_sets_non_pass_verdict() {
    let out = gate_fail(
        ".",
        GateKind::CiFast,
        mk_validate_output(false),
        vec![],
        vec![],
        ApiError {
            code: "gate.validate_failed".to_string(),
            message: "validate(ratchet) failed; gate aborted".to_string(),
        },
    );

    let verdict = out.verdict.expect("verdict");
    assert_ne!(verdict.decision.status, DecisionStatus::Pass);
    assert!(
        verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.validate_failed")
    );
}

#[test]
fn effective_receipt_contract_prefers_tool_contract() {
    let tool = ToolReceiptContract {
        min_duration_ms: Some(111),
        min_stdout_bytes: Some(222),
        expect_stdout_pattern: Some("ok".to_string()),
        expect_exit_codes: Some(vec![0]),
    };
    let qc = QualityContractConfig::default();
    let got = effective_receipt_contract(Some(&tool), Some(&qc)).expect("contract");
    assert_eq!(got.min_duration_ms, Some(111));
    assert_eq!(got.min_stdout_bytes, Some(222));
    assert_eq!(got.expect_stdout_pattern.as_deref(), Some("ok"));
    assert_eq!(got.expect_exit_codes, Some(vec![0]));
}

#[test]
fn effective_receipt_contract_falls_back_to_quality_defaults() {
    let qc = QualityContractConfig::default();
    let got = effective_receipt_contract(None, Some(&qc)).expect("fallback contract");
    assert_eq!(
        got.min_duration_ms,
        Some(qc.receipt_defaults.min_duration_ms)
    );
    assert_eq!(
        got.min_stdout_bytes,
        Some(qc.receipt_defaults.min_stdout_bytes)
    );
    assert!(got.expect_stdout_pattern.is_none());
    assert!(got.expect_exit_codes.is_none());
}

#[test]
fn effective_receipt_contract_none_without_tool_and_quality() {
    assert!(effective_receipt_contract(None, None).is_none());
}

#[test]
fn classify_run_failed_marks_not_found_as_non_transient() {
    let err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    assert_eq!(classify_run_failed(&err), "gate.run_failed");
}

#[test]
fn classify_run_failed_marks_timeout_as_transient() {
    let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout");
    assert_eq!(classify_run_failed(&err), "gate.run_failed_transient");
}

#[test]
fn required_tools_for_changes_maps_by_glob() {
    let mut qc = QualityContractConfig::default();
    qc.impact.rules = vec![ImpactRule {
        id: "src".to_string(),
        path_globs: vec!["src/**".to_string()],
        required_tools: vec!["cargo-test".to_string()],
    }];
    let (required, unmatched) =
        required_tools_for_changes(&qc, &["src/lib.rs".to_string(), "README.md".to_string()])
            .expect("map");
    assert!(required.contains("cargo-test"));
    assert_eq!(unmatched, vec!["README.md".to_string()]);
}

#[test]
fn receipt_contract_pattern_matches_stderr_tail() {
    let contract = ToolReceiptContract {
        min_duration_ms: None,
        min_stdout_bytes: None,
        expect_stdout_pattern: Some("READY".to_string()),
        expect_exit_codes: None,
    };
    let receipt = mk_receipt("no-match", "stderr says READY");
    let res = check_receipt_contract(&receipt, &contract);
    assert!(res.is_ok());
}

#[test]
fn receipt_contract_pattern_mismatch_reports_tail_lengths_and_bytes() {
    let contract = ToolReceiptContract {
        min_duration_ms: None,
        min_stdout_bytes: None,
        expect_stdout_pattern: Some("never-match".to_string()),
        expect_exit_codes: None,
    };
    let mut receipt = mk_receipt("alpha", "beta");
    receipt.stdout_bytes = 321;
    receipt.stderr_bytes = 654;

    let violation = check_receipt_contract(&receipt, &contract).expect_err("must fail");
    assert_eq!(violation.code, "gate.receipt_contract_violated");
    assert!(violation.message.contains("stdout_tail_len_bytes=5"));
    assert!(violation.message.contains("stderr_tail_len_bytes=4"));
    assert!(violation.message.contains("stdout_bytes=321"));
    assert!(violation.message.contains("stderr_bytes=654"));
}

#[test]
fn unmapped_path_violations_respect_policy() {
    let unmatched = vec!["README.md".to_string()];

    let block = unmapped_path_violations(ImpactUnmappedPathPolicy::Block, &unmatched);
    assert_eq!(block.len(), 1);
    assert_eq!(block[0].tier, ViolationTier::Blocking);

    let observe = unmapped_path_violations(ImpactUnmappedPathPolicy::Observe, &unmatched);
    assert_eq!(observe.len(), 1);
    assert_eq!(observe[0].tier, ViolationTier::Observation);

    let ignore = unmapped_path_violations(ImpactUnmappedPathPolicy::Ignore, &unmatched);
    assert!(ignore.is_empty());
}
