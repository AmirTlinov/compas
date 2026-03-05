use super::{Value, first_text, text};
use serde_json::json;

pub(super) fn normalize_compact_summary(raw: Option<&Value>, fallback: &str) -> String {
    raw.and_then(|value| {
        text(value).or_else(|| {
            first_text(value, &["compact", "summary", "message", "text", "title"])
                .map(|item| item.trim().to_string())
        })
    })
    .filter(|item| !item.trim().is_empty())
    .unwrap_or_else(|| fallback.to_string())
}

fn finding_digest(value: &Value) -> Option<String> {
    if let Some(text) = text(value) {
        return Some(text);
    }

    let code = first_text(value, &["code", "id", "rule_id", "ruleId", "name"]);
    let message = first_text(value, &["message", "summary", "title", "text"]);
    match (code, message) {
        (Some(code), Some(message)) if !message.eq_ignore_ascii_case(&code) => {
            Some(format!("{code}: {message}"))
        }
        (Some(code), _) => Some(code),
        (None, Some(message)) => Some(message),
        _ => None,
    }
}

pub(super) fn normalize_top_findings(raw: &[Value], findings_json: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = raw
        .iter()
        .filter_map(finding_digest)
        .filter(|item| !item.trim().is_empty())
        .fold(vec![], |mut acc, item| {
            if !acc.iter().any(|existing| existing == &item) {
                acc.push(item);
            }
            acc
        });

    if out.is_empty() {
        out = findings_json
            .iter()
            .filter_map(|item| {
                item.get("code")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
            .take(3)
            .collect();
    }

    out.truncate(3);
    out
}

fn remediation_item(value: &Value) -> Option<Value> {
    if let Some(item) = text(value) {
        return Some(Value::String(item));
    }

    let title = first_text(value, &["title", "summary", "message", "name"]);
    let steps = value
        .get("steps")
        .or_else(|| value.get("actions"))
        .or_else(|| value.get("instructions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(text)
        .collect::<Vec<_>>();

    if steps.is_empty() {
        title.map(Value::String)
    } else {
        Some(json!({
            "title": title.unwrap_or_else(|| "Remediation".to_string()),
            "steps": steps,
        }))
    }
}

pub(super) fn normalize_remediation(raw: &[Value]) -> Vec<Value> {
    let mut out = vec![];
    for item in raw.iter().filter_map(remediation_item) {
        if !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}
