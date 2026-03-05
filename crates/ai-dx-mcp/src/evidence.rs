use crate::api::{
    DecisionStatus, EvidenceEnvelope, EvidenceFinding, EvidenceSummary, GateOutput, ToolsRunOutput,
    ValidateOutput, Violation, ViolationTier,
};

mod report_bridge;

const MAX_FINDINGS: usize = 5;

fn find_top_codes(findings: &[EvidenceFinding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| f.code.clone())
        .filter(|code| !code.trim().is_empty())
        .take(3)
        .collect()
}

fn findings_from_violations(violations: &[Violation]) -> Vec<EvidenceFinding> {
    violations
        .iter()
        .take(MAX_FINDINGS)
        .map(|v| EvidenceFinding {
            code: v.code.clone(),
            message: v.message.clone(),
            path: v.path.clone(),
            tier: v.tier,
        })
        .collect()
}

fn blocking_from_findings(findings: &[EvidenceFinding]) -> bool {
    findings
        .iter()
        .any(|f| matches!(f.tier, ViolationTier::Blocking))
}

fn status_from_decision(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Pass => "pass",
        DecisionStatus::Retryable => "retryable",
        DecisionStatus::Blocked => "blocked",
    }
}

fn simple_cost_class(blocking: bool, findings_count: usize, artifacts_count: usize) -> String {
    if blocking || findings_count >= 12 || artifacts_count >= 6 {
        "high".to_string()
    } else if findings_count > 0 || artifacts_count > 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn remediation_from_validate(out: &ValidateOutput, blocking: bool) -> Vec<String> {
    if let Some(digest) = &out.agent_digest
        && !digest.minimal_fix_steps.is_empty()
    {
        return digest.minimal_fix_steps.iter().take(3).cloned().collect();
    }
    if !blocking {
        return vec!["keep current quality bars and continue with gate.".to_string()];
    }
    let code = out
        .violations
        .first()
        .map(|v| v.code.as_str())
        .unwrap_or("top-violation");
    vec![format!(
        "fix `{code}` and rerun compas.validate mode=ratchet."
    )]
}

pub(crate) fn build_validate_envelope(out: &ValidateOutput) -> EvidenceEnvelope {
    let findings = findings_from_violations(&out.violations);
    let status = out
        .verdict
        .as_ref()
        .map(|v| status_from_decision(v.decision.status).to_string())
        .unwrap_or_else(|| {
            if out.ok {
                "pass".to_string()
            } else {
                "blocked".to_string()
            }
        });
    let blocking = blocking_from_findings(&findings);
    let summary = EvidenceSummary {
        compact: format!(
            "validate status={status}; mode={:?}; violations={}; suppressed={}",
            out.mode,
            out.violations.len(),
            out.suppressed.len()
        ),
        top_findings: find_top_codes(&findings),
    };
    let remediation = remediation_from_validate(out, blocking);
    let cost_class = simple_cost_class(blocking, findings.len(), 0);
    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts: vec![],
        remediation,
        cost_class,
        evidence_ref: format!("validate:{}:{:?}", out.repo_root, out.mode),
    }
}

pub(crate) fn build_exec_envelope(out: &ToolsRunOutput) -> EvidenceEnvelope {
    report_bridge::build_exec_envelope(out)
}

pub(crate) fn build_gate_envelope(out: &GateOutput) -> EvidenceEnvelope {
    report_bridge::build_gate_envelope(out)
}
