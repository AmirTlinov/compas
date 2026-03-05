use super::{ParsedFinding, ParsedReport};
use regex::Regex;

fn xml_attr(input: &str, key: &str) -> Option<String> {
    let patterns = [format!("{key}=\""), format!("{key}='")];
    for pattern in patterns {
        let Some(start) = input.find(&pattern) else {
            continue;
        };
        let quote = pattern.chars().last().unwrap_or('"');
        let rest = &input[start + pattern.len()..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        let value = rest[..end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub(super) fn parse_junit_report(tool_id: &str, input: &str) -> Result<ParsedReport, String> {
    let testcase_re = Regex::new(r"(?s)<testcase\\b([^>]*)>(.*?)</testcase>")
        .map_err(|e| format!("tool={tool_id}: regex compile failed: {e}"))?;
    let event_re = Regex::new(r"(?s)<(failure|error)\\b([^>]*)>(.*?)</(?:failure|error)>")
        .map_err(|e| format!("tool={tool_id}: regex compile failed: {e}"))?;

    let mut findings = Vec::new();
    for case in testcase_re.captures_iter(input) {
        let attrs = case.get(1).map(|m| m.as_str()).unwrap_or_default();
        let inner = case.get(2).map(|m| m.as_str()).unwrap_or_default();
        let Some(event) = event_re.captures(inner) else {
            continue;
        };

        let class_name = xml_attr(attrs, "classname");
        let test_name = xml_attr(attrs, "name").unwrap_or_else(|| "testcase".to_string());
        let code = class_name
            .as_ref()
            .map(|class| format!("{class}.{test_name}"))
            .unwrap_or_else(|| test_name.clone());

        let event_attrs = event.get(2).map(|m| m.as_str()).unwrap_or_default();
        let event_body = event.get(3).map(|m| m.as_str()).unwrap_or_default();
        let event_tag = event.get(1).map(|m| m.as_str()).unwrap_or("failure");

        findings.push(ParsedFinding {
            code,
            category: Some("test".to_string()),
            message: xml_attr(event_attrs, "message")
                .or_else(|| {
                    let text = event_body.trim();
                    (!text.is_empty()).then_some(text.to_string())
                })
                .unwrap_or_else(|| "JUnit failure".to_string()),
            path: xml_attr(attrs, "file").or_else(|| class_name.clone()),
            line: xml_attr(attrs, "line").and_then(|n| n.parse::<u64>().ok()),
            severity_raw: event_tag.to_string(),
            evidence_ref: None,
        });
    }

    if findings.is_empty() {
        return Err(format!("tool={tool_id}: junit report has no failures"));
    }

    Ok(ParsedReport {
        findings,
        version: None,
        commit_sha: None,
        compact_summary_raw: None,
        top_findings_raw: vec![],
        remediation_raw: vec![],
    })
}
