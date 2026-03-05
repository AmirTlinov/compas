use serde_json::Value;

use super::{text, u64_value};

pub(super) fn project_summary(
    tool_id: &str,
    text_payload: &str,
    findings_json: &[Value],
    blocking_findings: usize,
) -> (String, Vec<String>, Vec<String>) {
    let fallback = format!(
        "tool={tool_id}; findings={}; blocking={blocking_findings}",
        findings_json.len()
    );
    let payload = serde_json::from_str::<Value>(text_payload).ok();
    (
        compact_summary_from_payload(tool_id, payload.as_ref(), &fallback, findings_json.len()),
        top_findings_from_payload(payload.as_ref(), findings_json),
        remediation_from_payload(payload.as_ref()),
    )
}

pub(super) fn compact_summary_from_payload(
    tool_id: &str,
    payload: Option<&Value>,
    fallback: &str,
    findings_count: usize,
) -> String {
    let Some(payload) = payload else {
        return fallback.to_string();
    };

    if let Some(summary) = payload.get("summary") {
        if let Some(compact) = summary.get("compact").and_then(text) {
            return compact;
        }
        if let Some(compact) = text(summary) {
            return compact;
        }
    }

    if let Some(compact_summary) = payload.get("compact_summary") {
        if let Some(compact) = compact_summary.get("compact").and_then(text) {
            return compact;
        }
        if let Some(reason) = compact_summary.get("status_reason").and_then(text) {
            let finding_count = compact_summary
                .get("finding_count")
                .and_then(u64_value)
                .unwrap_or(findings_count as u64);
            let error_count = compact_summary
                .get("error_count")
                .and_then(u64_value)
                .unwrap_or(0);
            return format!(
                "tool={tool_id}; status_reason={reason}; finding_count={finding_count}; error_count={error_count}"
            );
        }
    }

    fallback.to_string()
}

fn extract_top_finding_codes(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| {
            item.get("code")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .or_else(|| item.as_str().map(ToString::to_string))
        })
        .filter(|code| !code.trim().is_empty())
        .take(3)
        .collect()
}

pub(super) fn top_findings_from_payload(
    payload: Option<&Value>,
    fallback: &[Value],
) -> Vec<String> {
    if let Some(payload) = payload {
        if let Some(items) = payload
            .get("summary")
            .and_then(|summary| summary.get("top_findings"))
            .and_then(Value::as_array)
        {
            let out = extract_top_finding_codes(items);
            if !out.is_empty() {
                return out;
            }
        }

        if let Some(items) = payload.get("top_findings").and_then(Value::as_array) {
            let out = extract_top_finding_codes(items);
            if !out.is_empty() {
                return out;
            }
        }
    }

    fallback
        .iter()
        .filter_map(|item| {
            item.get("code")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .take(3)
        .collect()
}

pub(super) fn remediation_from_payload(payload: Option<&Value>) -> Vec<String> {
    let Some(items) = payload
        .and_then(|value| value.get("remediation"))
        .and_then(Value::as_array)
    else {
        return vec![];
    };

    let mut out: Vec<String> = vec![];
    for item in items {
        let text_item = if let Some(text) = item.as_str() {
            Some(text.trim().to_string())
        } else if let Some(obj) = item.as_object() {
            let title = obj.get("title").and_then(text);
            let steps: Vec<String> = obj
                .get("steps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(text)
                .take(2)
                .collect();
            match (title, steps.as_slice()) {
                (Some(title), [first, ..]) => Some(format!("{title}: {first}")),
                (Some(title), []) => Some(title),
                (None, [first, ..]) => Some(first.clone()),
                (None, []) => None,
            }
        } else {
            None
        };

        if let Some(text_item) = text_item
            && !text_item.is_empty()
            && !out.iter().any(|existing| existing == &text_item)
        {
            out.push(text_item);
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}
