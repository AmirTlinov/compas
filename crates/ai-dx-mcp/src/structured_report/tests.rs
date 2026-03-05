use super::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn ingest_json_report_preserves_adapter_summary_and_remediation() {
    let dir = tempdir().expect("tempdir");
    let report_path = dir.path().join("adapter-report.json");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&json!({
            "version": "1.0",
            "findings": [
                {
                    "code": "lint.alpha",
                    "severity": "high",
                    "message": "alpha failed",
                    "path": "src/lib.rs"
                },
                {
                    "code": "lint.beta",
                    "severity": "medium",
                    "message": "beta warning",
                    "path": "src/main.rs"
                }
            ],
            "compact_summary": "adapter summary wins",
            "top_findings": [
                {"code": "lint.beta", "message": "beta warning"},
                "lint.alpha"
            ],
            "remediation": [
                "run formatter",
                {
                    "title": "Fix imports",
                    "steps": ["sort imports", "rerun lint"]
                }
            ]
        }))
        .expect("serialize report"),
    )
    .expect("write report");

    let cfg = json!({
        "kind": "json",
        "path": "adapter-report.json"
    });

    let (report, violations) = ingest_tool_report(dir.path(), "demo-tool", &cfg);
    assert!(
        !violations.is_empty(),
        "findings should still emit violations"
    );

    let report = report.expect("structured report");
    assert_eq!(report["summary"]["compact"], "adapter summary wins");
    assert_eq!(report["compact_summary"], "adapter summary wins");
    assert_eq!(
        report["summary"]["top_findings"],
        json!(["lint.beta: beta warning", "lint.alpha"])
    );
    assert_eq!(
        report["top_findings"],
        json!(["lint.beta: beta warning", "lint.alpha"])
    );
    assert_eq!(report["remediation"][0], "run formatter");
    assert_eq!(report["remediation"][1]["title"], "Fix imports");
    assert_eq!(
        report["remediation"][1]["steps"],
        json!(["sort imports", "rerun lint"])
    );
}
