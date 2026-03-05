use super::*;
use crate::api::{
    AgentDigest, ApiError, BoundarySummary, CoverageSummary, Decision, DecisionReason,
    DecisionStatus, EffectiveConfigSummary, ErrorClass, FindingV2, GateKind, LocSummary,
    QualityPosture, Receipt, RiskSummary, TrustScore, ValidateMode, Verdict, ViolationTier,
    WitnessMeta,
};
use serde_json::{Value, json};

fn empty_validate_output() -> ValidateOutput {
    ValidateOutput {
        ok: true,
        error: None,
        schema_version: "1".to_string(),
        repo_root: "/repo".to_string(),
        mode: ValidateMode::Ratchet,
        violations: vec![],
        findings_v2: Vec::<FindingV2>::new(),
        suppressed: vec![],
        loc: None::<LocSummary>,
        boundary: None::<BoundarySummary>,
        public_surface: None,
        effective_config: None::<EffectiveConfigSummary>,
        risk_summary: None::<RiskSummary>,
        coverage: None::<CoverageSummary>,
        trust_score: None::<TrustScore>,
        verdict: Some(Verdict {
            decision: Decision {
                status: DecisionStatus::Pass,
                reasons: vec![],
                blocking_count: 0,
                observation_count: 0,
            },
            quality_posture: None::<QualityPosture>,
            suppressed_count: 0,
            suppressed_codes: vec![],
        }),
        quality_posture: None::<QualityPosture>,
        agent_digest: None::<AgentDigest>,
        summary_md: None,
        evidence: EvidenceEnvelope::default(),
        payload_meta: None,
    }
}

fn receipt_with_report(report: Value) -> Receipt {
    Receipt {
        tool_id: "demo-tool".to_string(),
        success: false,
        exit_code: Some(1),
        timed_out: false,
        duration_ms: 42,
        command: "demo".to_string(),
        args: vec!["--check".to_string()],
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        stdout_bytes: 0,
        stderr_bytes: 0,
        stdout_sha256: "stdout".to_string(),
        stderr_sha256: "stderr".to_string(),
        structured_report: Some(report),
    }
}

#[test]
fn build_exec_envelope_prefers_report_summary_and_remediation() {
    let out = ToolsRunOutput {
        ok: false,
        error: Some(ApiError {
            code: "compas.exec.exit_nonzero".to_string(),
            message: "tool failed".to_string(),
        }),
        repo_root: "/repo".to_string(),
        receipt: Some(receipt_with_report(json!({
            "findings": [
                {"code": "lint.alpha", "severity": "high", "message": "alpha failed"}
            ],
            "compact_summary": "adapter summary",
            "top_findings": ["lint.alpha", {"code": "lint.beta", "message": "beta warning"}],
            "remediation": [
                "run formatter",
                {"title": "Fix imports", "steps": ["sort imports", "rerun lint"]}
            ]
        }))),
        summary_md: None,
        evidence: EvidenceEnvelope::default(),
        payload_meta: None,
    };

    let envelope = build_exec_envelope(&out);
    assert_eq!(envelope.summary.compact, "adapter summary");
    assert_eq!(
        envelope.summary.top_findings,
        vec![
            "lint.alpha".to_string(),
            "lint.beta: beta warning".to_string()
        ]
    );
    assert_eq!(
        envelope.remediation,
        vec![
            "run formatter".to_string(),
            "Fix imports: sort imports; rerun lint".to_string()
        ]
    );
}

#[test]
fn build_exec_envelope_falls_back_without_report_summary() {
    let out = ToolsRunOutput {
        ok: false,
        error: None,
        repo_root: "/repo".to_string(),
        receipt: Some(receipt_with_report(json!({
            "findings": [
                {"code": "lint.alpha", "severity": "high", "message": "alpha failed"}
            ]
        }))),
        summary_md: None,
        evidence: EvidenceEnvelope::default(),
        payload_meta: None,
    };

    let envelope = build_exec_envelope(&out);
    assert_eq!(
        envelope.summary.compact,
        "exec status=blocked; tool_id=demo-tool; success=false; duration_ms=42"
    );
    assert_eq!(
        envelope.summary.top_findings,
        vec!["lint.alpha".to_string()]
    );
    assert_eq!(
        envelope.remediation,
        vec![
            "inspect receipt stderr_tail and structured_report, then rerun compas.exec."
                .to_string()
        ]
    );
}

#[test]
fn build_gate_envelope_prefers_report_summary_and_remediation() {
    let gate = GateOutput {
        ok: false,
        error: None,
        repo_root: "/repo".to_string(),
        kind: GateKind::CiFast,
        validate: empty_validate_output(),
        receipts: vec![receipt_with_report(json!({
            "findings": [
                {"code": "lint.alpha", "severity": "high", "message": "alpha failed"}
            ],
            "summary": {
                "compact": "report-backed gate summary",
                "top_findings": ["lint.alpha"]
            },
            "remediation": [
                "rerun the formatter",
                {"title": "Fix imports", "steps": ["sort imports", "rerun lint"]}
            ],
            "evidence": {
                "report_path": "target/report.json",
                "report_sha256": "abc123"
            }
        }))],
        witness_path: Some("target/witness.json".to_string()),
        witness: Some(WitnessMeta {
            path: "target/witness.json".to_string(),
            size_bytes: 12,
            sha256: "witness-sha".to_string(),
            rotated_files: 0,
        }),
        verdict: Some(Verdict {
            decision: Decision {
                status: DecisionStatus::Blocked,
                reasons: vec![DecisionReason {
                    code: "gate.tool_failed".to_string(),
                    class: ErrorClass::TransientTool,
                    tier: ViolationTier::Blocking,
                }],
                blocking_count: 1,
                observation_count: 0,
            },
            quality_posture: None::<QualityPosture>,
            suppressed_count: 0,
            suppressed_codes: vec![],
        }),
        agent_digest: Some(AgentDigest {
            top_blockers: vec!["gate.tool_failed".to_string()],
            root_causes: vec!["generic gate failure".to_string()],
            minimal_fix_steps: vec!["generic remediation".to_string()],
            confidence: "high".to_string(),
            suppressed_count: 0,
            suppressed_top_codes: vec![],
        }),
        summary_md: None,
        evidence: EvidenceEnvelope::default(),
        payload_meta: None,
        job: None,
        job_state: None,
        job_error: None,
    };

    let envelope = build_gate_envelope(&gate);
    assert_eq!(envelope.summary.compact, "report-backed gate summary");
    assert_eq!(
        envelope.summary.top_findings,
        vec!["lint.alpha".to_string()]
    );
    assert_eq!(
        envelope.remediation,
        vec![
            "rerun the formatter".to_string(),
            "Fix imports: sort imports; rerun lint".to_string()
        ]
    );
}
