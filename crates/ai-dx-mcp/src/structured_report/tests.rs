use super::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn ingest_tool_report_preserves_adapter_summary_top_findings_and_remediation() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    let report_path = repo.join("reports/custom.json");
    std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&json!({
            "findings": [
                {
                    "code": "lint.example",
                    "severity": "high",
                    "category": "lint",
                    "message": "Fix me",
                    "path": "src/lib.rs"
                }
            ],
            "compact_summary": {
                "finding_count": 1,
                "error_count": 0,
                "status_reason": "lint findings present"
            },
            "top_findings": [{ "code": "lint.example" }],
            "remediation": [
                {
                    "id": "lint.fix",
                    "title": "Apply lint fix",
                    "priority": "high",
                    "steps": ["run cargo fmt"]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let cfg = json!({
        "kind": "json",
        "path": "reports/custom.json",
        "required": true
    });
    let (report, violations) = ingest_tool_report(repo, "lint-tool", &cfg);
    assert_eq!(violations.len(), 1);
    let report = report.expect("report");
    assert_eq!(
        report
            .get("summary")
            .and_then(|v| v.get("compact"))
            .and_then(|v| v.as_str()),
        Some("tool=lint-tool; status_reason=lint findings present; finding_count=1; error_count=0")
    );
    assert_eq!(
        report
            .get("summary")
            .and_then(|v| v.get("top_findings"))
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            .and_then(|v| v.as_str()),
        Some("lint.example")
    );
    assert_eq!(
        report
            .get("remediation")
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            .and_then(|v| v.as_str()),
        Some("Apply lint fix: run cargo fmt")
    );
}

#[test]
fn ingest_tool_report_keeps_fallbacks_for_adapter_reports_without_summary_fields() {
    let dir = tempdir().unwrap();
    let repo = dir.path();
    let report_path = repo.join("reports/fallback.json");
    std::fs::create_dir_all(report_path.parent().unwrap()).unwrap();
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&json!({
            "findings": [
                {
                    "code": "compat.warn",
                    "severity": "medium",
                    "category": "lint",
                    "message": "Compatibility output"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let cfg = json!({
        "kind": "json",
        "path": "reports/fallback.json",
        "required": true
    });
    let (report, violations) = ingest_tool_report(repo, "compat-tool", &cfg);
    assert_eq!(violations.len(), 1);
    let report = report.expect("report");
    assert_eq!(
        report
            .get("summary")
            .and_then(|v| v.get("compact"))
            .and_then(|v| v.as_str()),
        Some("tool=compat-tool; findings=1; blocking=0")
    );
    assert_eq!(
        report
            .get("summary")
            .and_then(|v| v.get("top_findings"))
            .and_then(|v| v.as_array())
            .map(|items| items.len()),
        Some(1)
    );
    assert_eq!(
        report
            .get("remediation")
            .and_then(|v| v.as_array())
            .map(|items| items.len()),
        Some(0)
    );
}
