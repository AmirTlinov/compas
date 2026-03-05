use crate::api::{
    EvidenceArtifact, EvidenceEnvelope, EvidenceFinding, EvidenceSummary, GateKind, GateOutput,
    ToolsRunOutput, ViolationTier,
};
use serde_json::Value;

const MAX_FINDINGS: usize = 5;
const MAX_REMEDIATIONS: usize = 3;

fn find_top_codes(findings: &[EvidenceFinding]) -> Vec<String> {
    findings
        .iter()
        .map(|f| f.code.clone())
        .filter(|code| !code.trim().is_empty())
        .take(3)
        .collect()
}

fn blocking_from_findings(findings: &[EvidenceFinding]) -> bool {
    findings
        .iter()
        .any(|f| matches!(f.tier, ViolationTier::Blocking))
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

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for value in values {
        if value.trim().is_empty() || out.iter().any(|existing| existing == &value) {
            continue;
        }
        out.push(value);
    }
    out
}

fn extract_report_artifacts(report: &Value) -> Vec<EvidenceArtifact> {
    let mut out = vec![];
    let path = report
        .get("evidence")
        .and_then(|e| e.get("report_path"))
        .and_then(Value::as_str);
    let sha = report
        .get("evidence")
        .and_then(|e| e.get("report_sha256"))
        .and_then(Value::as_str);
    if let Some(path) = path {
        out.push(EvidenceArtifact {
            kind: "structured_report".to_string(),
            location: path.to_string(),
            sha256: sha.map(ToString::to_string),
        });
    }
    out
}

fn extract_report_findings(report: &Value) -> Vec<EvidenceFinding> {
    report
        .get("findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_FINDINGS)
        .map(|item| {
            let tier = match item
                .get("severity")
                .and_then(Value::as_str)
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
                    .and_then(Value::as_str)
                    .unwrap_or("tools.structured_report.finding")
                    .to_string(),
                message: item
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("structured report finding")
                    .to_string(),
                path: item
                    .get("path")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                tier,
            }
        })
        .collect()
}

fn extract_report_summary(report: &Value) -> Option<EvidenceSummary> {
    let compact = report
        .get("summary")
        .and_then(|summary| summary.get("compact"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)?;

    let top_findings = report
        .get("summary")
        .and_then(|summary| summary.get("top_findings"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .take(3)
        .collect();

    Some(EvidenceSummary {
        compact,
        top_findings,
    })
}

fn extract_report_remediation(report: &Value) -> Vec<String> {
    let items = report
        .get("remediation")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    dedupe_strings(
        items
            .into_iter()
            .filter_map(|item| {
                if let Some(text) = item
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    return Some(text.to_string());
                }

                let obj = item.as_object()?;
                let title = obj
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("Remediation");
                let steps: Vec<String> = obj
                    .get("steps")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .collect();
                if steps.is_empty() {
                    None
                } else {
                    Some(format!("{title}: {}", steps.join("; ")))
                }
            })
            .take(MAX_REMEDIATIONS)
            .collect(),
    )
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
    let report_summary = out
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.structured_report.as_ref())
        .and_then(extract_report_summary);
    let summary = if let Some(report_summary) = report_summary {
        let top_findings = if report_summary.top_findings.is_empty() {
            find_top_codes(&findings)
        } else {
            report_summary.top_findings
        };
        EvidenceSummary {
            compact: report_summary.compact,
            top_findings,
        }
    } else {
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
            top_findings: find_top_codes(&findings),
        }
    };
    let remediation = out
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.structured_report.as_ref())
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
    let mut findings = out
        .validate
        .violations
        .iter()
        .take(MAX_FINDINGS)
        .map(|v| EvidenceFinding {
            code: v.code.clone(),
            message: v.message.clone(),
            path: v.path.clone(),
            tier: v.tier,
        })
        .collect::<Vec<_>>();
    let mut artifacts: Vec<EvidenceArtifact> = vec![];
    let mut primary_report_summary: Option<EvidenceSummary> = None;
    let mut report_remediation: Vec<String> = vec![];

    for receipt in &out.receipts {
        if let Some(report) = &receipt.structured_report {
            findings.extend(extract_report_findings(report));
            artifacts.extend(extract_report_artifacts(report));
            if primary_report_summary.is_none() {
                primary_report_summary = extract_report_summary(report);
            }
            report_remediation.extend(extract_report_remediation(report));
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
        .map(|v| match v.decision.status {
            crate::api::DecisionStatus::Pass => "pass".to_string(),
            crate::api::DecisionStatus::Retryable => "retryable".to_string(),
            crate::api::DecisionStatus::Blocked => "blocked".to_string(),
        })
        .unwrap_or_else(|| {
            if out.ok {
                "pass".to_string()
            } else {
                "blocked".to_string()
            }
        });
    let blocking = blocking_from_findings(&findings) || !out.ok;
    let summary = if let Some(report_summary) = primary_report_summary {
        let top_findings = if report_summary.top_findings.is_empty() {
            find_top_codes(&findings)
        } else {
            report_summary.top_findings
        };
        EvidenceSummary {
            compact: format!(
                "gate kind={}; validate_violations={}; report={}",
                gate_kind_slug(out.kind),
                out.validate.violations.len(),
                report_summary.compact
            ),
            top_findings,
        }
    } else {
        EvidenceSummary {
            compact: format!(
                "gate status={status}; kind={}; receipts={}; validate_violations={}",
                gate_kind_slug(out.kind),
                out.receipts.len(),
                out.validate.violations.len()
            ),
            top_findings: find_top_codes(&findings),
        }
    };
    let remediation = if report_remediation.is_empty() {
        remediation_from_gate(out, blocking)
    } else {
        dedupe_strings(report_remediation)
            .into_iter()
            .take(MAX_REMEDIATIONS)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ApiError, PayloadMeta, Receipt, ResponseMode, ValidateMode, ValidateOutput};
    use std::collections::BTreeMap;

    fn mk_validate() -> ValidateOutput {
        ValidateOutput {
            ok: true,
            error: None,
            schema_version: "4".to_string(),
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
            evidence: EvidenceEnvelope::default(),
            payload_meta: Some(PayloadMeta {
                mode: ResponseMode::Compact,
                truncated: false,
                omitted: BTreeMap::new(),
            }),
        }
    }

    fn mk_receipt_with_report(report: Value) -> Receipt {
        Receipt {
            tool_id: "structured".to_string(),
            success: true,
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 10,
            command: "python3".to_string(),
            args: vec![],
            stdout_tail: "{}".to_string(),
            stderr_tail: String::new(),
            stdout_bytes: 2,
            stderr_bytes: 0,
            stdout_sha256: "a".repeat(64),
            stderr_sha256: "b".repeat(64),
            structured_report: Some(report),
        }
    }

    #[test]
    fn build_exec_envelope_prefers_structured_report_summary_and_remediation() {
        let out = ToolsRunOutput {
            ok: false,
            error: Some(ApiError {
                code: "tools.structured_report.blocking_findings".to_string(),
                message: "blocking findings".to_string(),
            }),
            repo_root: ".".to_string(),
            receipt: Some(mk_receipt_with_report(serde_json::json!({
                "findings": [
                    { "code": "lint.example", "severity": "high", "message": "Fix me", "path": "src/lib.rs" }
                ],
                "summary": {
                    "compact": "lint findings present",
                    "top_findings": ["lint.example"]
                },
                "remediation": [
                    {
                        "id": "fix-lint",
                        "title": "Clean up lint",
                        "priority": "high",
                        "steps": ["Remove unused import", "Rerun lint"]
                    }
                ],
                "evidence": {
                    "report_path": "reports/custom.json",
                    "report_sha256": "c".repeat(64)
                }
            }))),
            summary_md: None,
            evidence: EvidenceEnvelope::default(),
            payload_meta: None,
        };

        let envelope = build_exec_envelope(&out);
        assert_eq!(envelope.summary.compact, "lint findings present");
        assert_eq!(
            envelope.summary.top_findings,
            vec!["lint.example".to_string()]
        );
        assert_eq!(
            envelope.remediation,
            vec!["Clean up lint: Remove unused import; Rerun lint".to_string()]
        );
    }

    #[test]
    fn build_exec_envelope_keeps_deterministic_fallback_for_legacy_reports() {
        let out = ToolsRunOutput {
            ok: false,
            error: None,
            repo_root: ".".to_string(),
            receipt: Some(mk_receipt_with_report(serde_json::json!({
                "findings": [
                    { "code": "legacy.warn", "severity": "medium", "message": "Legacy warning" }
                ],
                "evidence": {
                    "report_path": "reports/legacy.json",
                    "report_sha256": "d".repeat(64)
                }
            }))),
            summary_md: None,
            evidence: EvidenceEnvelope::default(),
            payload_meta: None,
        };

        let envelope = build_exec_envelope(&out);
        assert_eq!(
            envelope.summary.compact,
            "exec status=blocked; tool_id=structured; success=true; duration_ms=10"
        );
        assert_eq!(
            envelope.summary.top_findings,
            vec!["legacy.warn".to_string()]
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
    fn build_gate_envelope_surfaces_structured_report_context() {
        let out = GateOutput {
            ok: false,
            error: None,
            repo_root: ".".to_string(),
            kind: GateKind::CiFast,
            validate: mk_validate(),
            receipts: vec![mk_receipt_with_report(serde_json::json!({
                "findings": [
                    { "code": "lint.example", "severity": "high", "message": "Fix me", "path": "src/lib.rs" }
                ],
                "summary": {
                    "compact": "lint findings present",
                    "top_findings": ["lint.example"]
                },
                "remediation": [
                    {
                        "id": "fix-lint",
                        "title": "Clean up lint",
                        "priority": "high",
                        "steps": ["Remove unused import", "Rerun lint"]
                    }
                ],
                "evidence": {
                    "report_path": "reports/custom.json",
                    "report_sha256": "c".repeat(64)
                }
            }))],
            witness_path: None,
            witness: None,
            verdict: None,
            agent_digest: None,
            summary_md: None,
            evidence: EvidenceEnvelope::default(),
            payload_meta: None,
            job: None,
            job_state: None,
            job_error: None,
        };

        let envelope = build_gate_envelope(&out);
        assert_eq!(
            envelope.summary.compact,
            "gate kind=ci_fast; validate_violations=0; report=lint findings present"
        );
        assert_eq!(
            envelope.summary.top_findings,
            vec!["lint.example".to_string()]
        );
        assert_eq!(
            envelope.remediation,
            vec!["Clean up lint: Remove unused import; Rerun lint".to_string()]
        );
    }
}
