use serde_json::Value;

pub(super) fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub(super) fn u64_value(v: &Value) -> Option<u64> {
    if let Some(v) = v.as_u64() {
        return Some(v);
    }
    if let Some(v) = v.as_i64() {
        return (v >= 0).then_some(v as u64);
    }
    v.as_str().and_then(|s| s.parse::<u64>().ok())
}

pub(super) fn find_json_path<'a>(root: &'a Value, dotted_path: &str) -> Option<&'a Value> {
    let mut current = root;
    for part in dotted_path
        .split('.')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

pub(super) fn message(v: &Value) -> String {
    [
        v.get("message").and_then(text),
        v.get("msg").and_then(text),
        v.get("text").and_then(text),
        v.get("message").and_then(|m| m.get("text")).and_then(text),
    ]
    .into_iter()
    .flatten()
    .next()
    .unwrap_or_else(|| "<empty message>".to_string())
}

pub(super) fn first_text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| v.get(*key).and_then(text))
}
