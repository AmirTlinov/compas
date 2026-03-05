use crate::api::{
    DecisionStatus, EvidenceArtifact, EvidenceEnvelope, EvidenceFinding, EvidenceSummary, GateKind,
    GateOutput, ToolsRunOutput, ValidateOutput, Violation, ViolationTier,
};

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

fn remediation_from_gate(out: &GateOutput, blocking: bool) -> Vec<String> {
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

fn remediation_from_exec(out: &ToolsRunOutput, blocking: bool) -> Vec<String> {
    if !blocking {
        return vec!["tool execution succeeded; continue with gate.".to_string()];
    }
    let fallback = "inspect receipt stderr_tail and structured_report, then rerun compas.exec.";
    out.error
        .as_ref()
        .map(|err| vec![format!("fix `{}` and rerun compas.exec.", err.code)])
        .unwrap_or_else(|| vec![fallback.to_string()])
}

fn gate_kind_slug(kind: GateKind) -> &'static str {
    match kind {
        GateKind::CiFast => "ci_fast",
        GateKind::Ci => "ci",
        GateKind::Flagship => "flagship",
    }
}

fn extract_report_artifacts(report: &serde_json::Value) -> Vec<EvidenceArtifact> {
    let mut out = vec![];
    let path = report
        .get("evidence")
        .and_then(|e| e.get("report_path"))
        .and_then(|v| v.as_str());
    let sha = report
        .get("evidence")
        .and_then(|e| e.get("report_sha256"))
        .and_then(|v| v.as_str());
    if let Some(path) = path {
        out.push(EvidenceArtifact {
            kind: "structured_report".to_string(),
            location: path.to_string(),
            sha256: sha.map(ToString::to_string),
        });
    }
    out
}

fn extract_report_findings(report: &serde_json::Value) -> Vec<EvidenceFinding> {
    report
        .get("findings")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .take(MAX_FINDINGS)
        .map(|item| {
            let tier = match item
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "critical" | "high" => ViolationTier::Blocking,
                _ => ViolationTier::Observation,
            };
            EvidenceFinding {
                code: item
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tools.structured_report.finding")
                    .to_string(),
                message: item
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("structured report finding")
                    .to_string(),
                path: item
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string),
                tier,
            }
        })
        .collect()
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
    let mut findings: Vec<EvidenceFinding> = vec![];
    let mut artifacts: Vec<EvidenceArtifact> = vec![];
    let mut status = if out.ok {
        "pass".to_string()
    } else {
        "blocked".to_string()
    };

    if let Some(error) = &out.error {
        findings.push(EvidenceFinding {
            code: error.code.clone(),
            message: error.message.clone(),
            path: None,
            tier: ViolationTier::Blocking,
        });
    }

    if let Some(receipt) = &out.receipt {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
        }
        if !receipt.success && out.error.is_none() {
            findings.push(EvidenceFinding {
                code: "compas.exec.exit_nonzero".to_string(),
                message: format!("tool {} failed", receipt.tool_id),
                path: None,
                tier: ViolationTier::Blocking,
            });
            status = "blocked".to_string();
        }
    }

    findings.truncate(MAX_FINDINGS);
    let blocking = blocking_from_findings(&findings) || !out.ok;
    let top_findings = find_top_codes(&findings);
    let summary = EvidenceSummary {
        compact: out
            .receipt
            .as_ref()
            .map(|r| {
                format!(
                    "exec status={status}; tool_id={}; success={}; duration_ms={}",
                    r.tool_id, r.success, r.duration_ms
                )
            })
            .unwrap_or_else(|| format!("exec status={status}; receipt=missing")),
        top_findings,
    };
    let remediation = remediation_from_exec(out, blocking);
    let cost_class = simple_cost_class(blocking, findings.len(), artifacts.len());
    let evidence_ref = out
        .receipt
        .as_ref()
        .map(|r| format!("exec:{}:{}", out.repo_root, r.tool_id))
        .unwrap_or_else(|| format!("exec:{}", out.repo_root));

    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts,
        remediation,
        cost_class,
        evidence_ref,
    }
}

pub(crate) fn build_gate_envelope(out: &GateOutput) -> EvidenceEnvelope {
    let mut findings = findings_from_violations(&out.validate.violations);
    let mut artifacts: Vec<EvidenceArtifact> = vec![];

    for receipt in &out.receipts {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
        }
        if !receipt.success {
            findings.push(EvidenceFinding {
                code: "gate.tool_failed".to_string(),
                message: format!("tool {} failed inside gate", receipt.tool_id),
                path: None,
                tier: ViolationTier::Blocking,
            });
        }
    }

    if let Some(meta) = &out.witness {
        artifacts.push(EvidenceArtifact {
            kind: "witness".to_string(),
            location: meta.path.clone(),
            sha256: Some(meta.sha256.clone()),
        });
    } else if let Some(path) = &out.witness_path {
        artifacts.push(EvidenceArtifact {
            kind: "witness".to_string(),
            location: path.clone(),
            sha256: None,
        });
    }

    findings.truncate(MAX_FINDINGS);
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
    let blocking = blocking_from_findings(&findings) || !out.ok;
    let summary = EvidenceSummary {
        compact: format!(
            "gate status={status}; kind={}; receipts={}; validate_violations={}",
            gate_kind_slug(out.kind),
            out.receipts.len(),
            out.validate.violations.len()
        ),
        top_findings: find_top_codes(&findings),
    };
    let remediation = remediation_from_gate(out, blocking);
    let cost_class = simple_cost_class(blocking, findings.len(), artifacts.len());
    let evidence_ref = out
        .witness
        .as_ref()
        .map(|w| format!("gate:{}:{}", gate_kind_slug(out.kind), w.sha256))
        .unwrap_or_else(|| format!("gate:{}:{}", gate_kind_slug(out.kind), out.repo_root));

    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts,
        remediation,
        cost_class,
        evidence_ref,
    }
}
