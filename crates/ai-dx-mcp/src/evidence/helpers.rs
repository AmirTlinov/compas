use crate::api::{
    DecisionStatus, EvidenceFinding, GateKind, GateOutput, ToolsRunOutput, ValidateOutput,
    ViolationTier,
};

pub(super) fn find_top_codes(findings: &[EvidenceFinding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| f.code.clone())
        .filter(|code| !code.trim().is_empty())
        .take(3)
        .collect()
}

pub(super) fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for value in values {
        if value.trim().is_empty() || out.iter().any(|existing| existing == &value) {
            continue;
        }
        out.push(value);
    }
    out
}

pub(super) fn blocking_from_findings(findings: &[EvidenceFinding]) -> bool {
    findings
        .iter()
        .any(|f| matches!(f.tier, ViolationTier::Blocking))
}

pub(super) fn status_from_decision(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Pass => "pass",
        DecisionStatus::Retryable => "retryable",
        DecisionStatus::Blocked => "blocked",
    }
}

pub(super) fn simple_cost_class(
    blocking: bool,
    findings_count: usize,
    artifacts_count: usize,
) -> String {
    if blocking || findings_count >= 12 || artifacts_count >= 6 {
        "high".to_string()
    } else if findings_count > 0 || artifacts_count > 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

pub(super) fn remediation_from_validate(out: &ValidateOutput, blocking: bool) -> Vec<String> {
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

pub(super) fn remediation_from_gate(out: &GateOutput, blocking: bool) -> Vec<String> {
    if let Some(digest) = &out.agent_digest
        && !digest.minimal_fix_steps.is_empty()
    {
        return digest.minimal_fix_steps.iter().take(3).cloned().collect();
    }
    if !blocking {
        return vec!["gate passed; continue delivery.".to_string()];
    }
    vec!["fix blocking findings and rerun compas.gate kind=ci_fast.".to_string()]
}

pub(super) fn remediation_from_exec(out: &ToolsRunOutput, blocking: bool) -> Vec<String> {
    if !blocking {
        return vec!["tool execution succeeded; continue with gate.".to_string()];
    }
    let fallback = "inspect receipt stderr_tail and structured_report, then rerun compas.exec.";
    out.error
        .as_ref()
        .map(|err| vec![format!("fix `{}` and rerun compas.exec.", err.code)])
        .unwrap_or_else(|| vec![fallback.to_string()])
}

pub(super) fn gate_kind_slug(kind: GateKind) -> &'static str {
    match kind {
        GateKind::CiFast => "ci_fast",
        GateKind::Ci => "ci",
        GateKind::Flagship => "flagship",
    }
}
