use crate::api::{
    EvidenceArtifact, EvidenceEnvelope, EvidenceFinding, EvidenceSummary, GateOutput,
    ToolsRunOutput, ValidateOutput, Violation,
};
use serde_json::Value;

mod helpers;
mod report_extract;
#[cfg(test)]
mod tests;

use helpers::{
    blocking_from_findings, find_top_codes, gate_kind_slug, remediation_from_exec,
    remediation_from_gate, remediation_from_validate, simple_cost_class, status_from_decision,
};
use report_extract::{
    extract_report_artifacts, extract_report_findings, extract_report_remediation,
    extract_report_summary, fill_summary_fallbacks,
};

const MAX_FINDINGS: usize = 5;

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

    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts: vec![],
        remediation: remediation_from_validate(out, blocking),
        cost_class: simple_cost_class(blocking, out.violations.len(), 0),
        evidence_ref: format!("validate:{}:{:?}", out.repo_root, out.mode),
    }
}

pub(crate) fn build_exec_envelope(out: &ToolsRunOutput) -> EvidenceEnvelope {
    let mut findings = vec![];
    let mut artifacts = vec![];
    let mut status = if out.ok {
        "pass".to_string()
    } else {
        "blocked".to_string()
    };
    let mut preferred_summary: Option<EvidenceSummary> = None;
    let mut preferred_remediation = vec![];
    let mut report_ref: Option<&Value> = None;

    if let Some(error) = &out.error {
        findings.push(EvidenceFinding {
            code: error.code.clone(),
            message: error.message.clone(),
            path: None,
            tier: crate::api::ViolationTier::Blocking,
        });
    }

    if let Some(receipt) = &out.receipt {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
            preferred_summary = extract_report_summary(report);
            preferred_remediation = extract_report_remediation(report);
            report_ref = Some(report);
        }
        if !receipt.success && out.error.is_none() {
            findings.push(EvidenceFinding {
                code: "compas.exec.exit_nonzero".to_string(),
                message: format!("tool {} failed", receipt.tool_id),
                path: None,
                tier: crate::api::ViolationTier::Blocking,
            });
            status = "blocked".to_string();
        }
    }

    findings.truncate(MAX_FINDINGS);
    let blocking = blocking_from_findings(&findings) || !out.ok;
    let mut summary = preferred_summary.unwrap_or_else(|| EvidenceSummary {
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
        top_findings: vec![],
    });
    fill_summary_fallbacks(&mut summary, report_ref, &findings);

    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts: artifacts.clone(),
        remediation: if preferred_remediation.is_empty() {
            remediation_from_exec(out, blocking)
        } else {
            preferred_remediation
        },
        cost_class: simple_cost_class(blocking, out.receipt.iter().count(), artifacts.len()),
        evidence_ref: out
            .receipt
            .as_ref()
            .map(|r| format!("exec:{}:{}", out.repo_root, r.tool_id))
            .unwrap_or_else(|| format!("exec:{}", out.repo_root)),
    }
}

pub(crate) fn build_gate_envelope(out: &GateOutput) -> EvidenceEnvelope {
    let mut findings = findings_from_violations(&out.validate.violations);
    let mut artifacts: Vec<EvidenceArtifact> = vec![];
    let mut preferred_summary: Option<EvidenceSummary> = None;
    let mut preferred_remediation = vec![];
    let mut summary_report_ref: Option<&Value> = None;

    for receipt in &out.receipts {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
            if preferred_summary.is_none() {
                preferred_summary = extract_report_summary(report);
                summary_report_ref = Some(report);
            }
            preferred_remediation.extend(extract_report_remediation(report));
        }
        if !receipt.success {
            findings.push(EvidenceFinding {
                code: "gate.tool_failed".to_string(),
                message: format!("tool {} failed inside gate", receipt.tool_id),
                path: None,
                tier: crate::api::ViolationTier::Blocking,
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
    let mut summary = preferred_summary.unwrap_or_else(|| EvidenceSummary {
        compact: format!(
            "gate status={status}; kind={}; receipts={}; validate_violations={}",
            gate_kind_slug(out.kind),
            out.receipts.len(),
            out.validate.violations.len()
        ),
        top_findings: vec![],
    });
    fill_summary_fallbacks(&mut summary, summary_report_ref, &findings);

    EvidenceEnvelope {
        status,
        blocking,
        findings,
        summary,
        artifacts: artifacts.clone(),
        remediation: if preferred_remediation.is_empty() {
            remediation_from_gate(out, blocking)
        } else {
            helpers::dedupe_strings(preferred_remediation)
                .into_iter()
                .take(3)
                .collect()
        },
        cost_class: simple_cost_class(blocking, out.receipts.len(), artifacts.len()),
        evidence_ref: out
            .witness
            .as_ref()
            .map(|w| format!("gate:{}:{}", gate_kind_slug(out.kind), w.sha256))
            .unwrap_or_else(|| format!("gate:{}:{}", gate_kind_slug(out.kind), out.repo_root)),
    }
}
