use crate::api::{EvidenceArtifact, EvidenceFinding, EvidenceSummary, ViolationTier};
use serde_json::Value;

use super::helpers::{dedupe_strings, find_top_codes};

pub(super) fn extract_report_artifacts(report: &Value) -> Vec<EvidenceArtifact> {
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

pub(super) fn extract_report_findings(report: &Value) -> Vec<EvidenceFinding> {
    report
        .get("findings")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .take(5)
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

pub(super) fn extract_report_finding_codes(report: &Value) -> Vec<String> {
    report
        .get("findings")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("code").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(ToString::to_string)
        .take(3)
        .collect()
}

pub(super) fn extract_report_summary(report: &Value) -> Option<EvidenceSummary> {
    let compact = report
        .get("summary")
        .and_then(|s| s.get("compact"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)?;

    let top_findings = report
        .get("summary")
        .and_then(|s| s.get("top_findings"))
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .take(3)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| extract_report_finding_codes(report));

    Some(EvidenceSummary {
        compact,
        top_findings,
    })
}

pub(super) fn fill_summary_fallbacks(
    summary: &mut EvidenceSummary,
    report: Option<&Value>,
    findings: &[EvidenceFinding],
) {
    if summary.top_findings.is_empty() {
        summary.top_findings = report
            .map(extract_report_finding_codes)
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| find_top_codes(findings));
    }
}

pub(super) fn extract_report_remediation(report: &Value) -> Vec<String> {
    let items = report
        .get("remediation")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    dedupe_strings(
        items
            .into_iter()
            .filter_map(|item| {
                if let Some(text) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    return Some(text.to_string());
                }

                let obj = item.as_object()?;
                let title = obj
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Remediation");
                let steps: Vec<String> = obj
                    .get("steps")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect();
                if steps.is_empty() {
                    return None;
                }
                Some(format!("{title}: {}", steps.join("; ")))
            })
            .take(3)
            .collect(),
    )
}
