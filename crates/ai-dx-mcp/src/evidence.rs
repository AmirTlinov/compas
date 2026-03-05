use crate::api::{
    EvidenceEnvelope, EvidenceFinding, EvidenceSummary, GateOutput, ToolsRunOutput, ValidateOutput,
    Violation,
};

mod helpers;
mod report_extract;
#[cfg(test)]
mod tests;

use helpers::{
    blocking_from_findings, dedupe_strings, find_top_codes, gate_kind_slug, remediation_from_exec,
    remediation_from_gate, remediation_from_validate, simple_cost_class, status_from_decision,
};
use report_extract::{
    extract_report_artifacts, extract_report_finding_codes, extract_report_findings,
    extract_report_remediation, extract_report_summary, fill_summary_fallbacks,
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
    let mut artifacts = vec![];
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
            tier: crate::api::ViolationTier::Blocking,
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
                tier: crate::api::ViolationTier::Blocking,
            });
            status = "blocked".to_string();
        }
    }

    findings.truncate(MAX_FINDINGS);
    let blocking = blocking_from_findings(&findings) || !out.ok;
    let report_summary = out
        .receipt
        .as_ref()
        .and_then(|r| r.structured_report.as_ref())
        .and_then(extract_report_summary);
    let summary = if let Some(mut report_summary) = report_summary {
        let report = out
            .receipt
            .as_ref()
            .and_then(|r| r.structured_report.as_ref());
        fill_summary_fallbacks(&mut report_summary, report, &findings);
        report_summary
    } else {
        let report_top_findings = out
            .receipt
            .as_ref()
            .and_then(|r| r.structured_report.as_ref())
            .map(extract_report_finding_codes)
            .filter(|items| !items.is_empty());
        EvidenceSummary {
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
            top_findings: report_top_findings.unwrap_or_else(|| find_top_codes(&findings)),
        }
    };
    let remediation = out
        .receipt
        .as_ref()
        .and_then(|r| r.structured_report.as_ref())
        .map(extract_report_remediation)
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| remediation_from_exec(out, blocking));
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
    let mut artifacts = vec![];
    let mut primary_report_summary: Option<EvidenceSummary> = None;
    let mut primary_report_top_findings: Vec<String> = vec![];
    let mut report_remediation = vec![];

    for receipt in &out.receipts {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
            if primary_report_summary.is_none() {
                primary_report_summary = extract_report_summary(report);
                primary_report_top_findings = extract_report_finding_codes(report);
            }
            report_remediation.extend(extract_report_remediation(report));
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
        artifacts.push(crate::api::EvidenceArtifact {
            kind: "witness".to_string(),
            location: meta.path.clone(),
            sha256: Some(meta.sha256.clone()),
        });
    } else if let Some(path) = &out.witness_path {
        artifacts.push(crate::api::EvidenceArtifact {
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
    let report_suffix = primary_report_summary
        .as_ref()
        .map(|summary| format!("; report={}", summary.compact))
        .unwrap_or_default();
    let top_findings = primary_report_summary
        .map(|mut summary| {
            fill_summary_fallbacks(&mut summary, None, &findings);
            summary.top_findings
        })
        .filter(|items| !items.is_empty())
        .or_else(|| {
            (!primary_report_top_findings.is_empty()).then_some(primary_report_top_findings)
        })
        .unwrap_or_else(|| find_top_codes(&findings));
    let summary = EvidenceSummary {
        compact: format!(
            "gate status={status}; kind={}; receipts={}; validate_violations={}{}",
            gate_kind_slug(out.kind),
            out.receipts.len(),
            out.validate.violations.len(),
            report_suffix,
        ),
        top_findings,
    };
    let remediation = if report_remediation.is_empty() {
        remediation_from_gate(out, blocking)
    } else {
        dedupe_strings(report_remediation)
            .into_iter()
            .take(3)
            .collect()
    };
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
