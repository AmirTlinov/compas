use crate::api::Violation;
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Component, Path};

const ALLOWLIST_REL_PATH: &str = ".agents/mcp/compas/allowlist.toml";

pub struct SuppressionResult {
    pub violations: Vec<Violation>,
    pub suppressed: Vec<Violation>,
}

#[derive(Debug, Deserialize)]
struct AllowlistFile {
    #[serde(default)]
    exceptions: Vec<ExceptionEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExceptionEntry {
    id: String,
    rule: String,
    path: String,
    owner: String,
    reason: String,
    expires_at: Option<String>,
}

fn normalize_exception_path(raw: &str) -> String {
    raw.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn has_glob_chars(s: &str) -> bool {
    s.contains('*')
        || s.contains('?')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}')
}

fn is_relative_and_safe(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return false;
    }
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return false,
            _ => {}
        }
    }
    true
}

fn allowlist_invalid(path: &str, msg: impl Into<String>) -> Violation {
    Violation::blocking(
        "exception.allowlist_invalid",
        msg.into(),
        Some(path.to_string()),
        None,
    )
}

fn expired_exception(path: &str, entry: &ExceptionEntry) -> Violation {
    Violation::blocking(
        "exception.expired",
        format!(
            "allowlist exception expired: id={} rule={} path={} expires_at={}",
            entry.id,
            entry.rule,
            entry.path,
            entry.expires_at.as_deref().unwrap_or("<missing>")
        ),
        Some(path.to_string()),
        None,
    )
}

fn window_exceeded_exception(
    path: &str,
    entry: &ExceptionEntry,
    max_days: u32,
    days_ahead: i64,
) -> Violation {
    Violation::blocking(
        "exception.window_exceeded",
        format!(
            "allowlist exception window exceeds max_exception_window_days: id={} rule={} path={} expires_at={} days_ahead={} max_days={}",
            entry.id,
            entry.rule,
            entry.path,
            entry.expires_at.as_deref().unwrap_or("<missing>"),
            days_ahead,
            max_days
        ),
        Some(path.to_string()),
        None,
    )
}

pub fn apply_allowlist_with_limits(
    repo_root: &Path,
    input: Vec<Violation>,
    max_exception_window_days: Option<u32>,
) -> SuppressionResult {
    let allowlist_rel_path = ALLOWLIST_REL_PATH;
    let allowlist_path = repo_root.join(ALLOWLIST_REL_PATH);
    if !allowlist_path.is_file() {
        return SuppressionResult {
            violations: input,
            suppressed: vec![],
        };
    }

    let invalid = |msg| allowlist_invalid(allowlist_rel_path, msg);

    let raw = match std::fs::read_to_string(&allowlist_path) {
        Ok(s) => s,
        Err(e) => {
            let mut violations = vec![invalid(format!(
                "failed to read allowlist {:?}: {e}",
                allowlist_path
            ))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
    };

    let parsed: AllowlistFile = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            let mut violations = vec![invalid(format!(
                "failed to parse allowlist {:?}: {e}",
                allowlist_path
            ))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
    };

    let today = Utc::now().date_naive();

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut entries: Vec<ExceptionEntry> = vec![];
    let mut expired: Vec<Violation> = vec![];

    for mut e in parsed.exceptions {
        e.id = e.id.trim().to_string();
        e.rule = e.rule.trim().to_string();
        e.path = normalize_exception_path(&e.path);
        e.owner = e.owner.trim().to_string();
        e.reason = e.reason.trim().to_string();

        if e.id.is_empty() {
            let mut violations = vec![invalid("exception entry has empty id".to_string())];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
        if !seen_ids.insert(e.id.clone()) {
            let mut violations = vec![invalid(format!(
                "duplicate exception id={} (ids must be unique)",
                e.id
            ))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }

        if e.rule.is_empty() {
            let mut violations = vec![invalid(format!("exception id={} has empty rule", e.id))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
        if e.path.is_empty() {
            let mut violations = vec![invalid(format!("exception id={} has empty path", e.id))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
        if !is_relative_and_safe(&e.path) {
            let mut violations = vec![invalid(format!(
                "exception id={} has unsafe/absolute path={}",
                e.id, e.path
            ))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
        if has_glob_chars(&e.path) {
            let mut violations = vec![invalid(format!(
                "exception id={} uses glob characters in path (globs are forbidden): {}",
                e.id, e.path
            ))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }

        if e.owner.is_empty() {
            let mut violations = vec![invalid(format!("exception id={} has empty owner", e.id))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }
        if e.reason.is_empty() {
            let mut violations = vec![invalid(format!("exception id={} has empty reason", e.id))];
            violations.extend(input);
            return SuppressionResult {
                violations,
                suppressed: vec![],
            };
        }

        if let Some(expires_at) = &e.expires_at {
            let expires_at = expires_at.trim();
            let expires_date = match NaiveDate::parse_from_str(expires_at, "%Y-%m-%d") {
                Ok(d) => d,
                Err(err) => {
                    let mut violations = vec![invalid(format!(
                        "exception id={} has invalid expires_at={expires_at:?}: {err}",
                        e.id
                    ))];
                    violations.extend(input);
                    return SuppressionResult {
                        violations,
                        suppressed: vec![],
                    };
                }
            };

            if expires_date < today {
                expired.push(expired_exception(allowlist_rel_path, &e));
                continue;
            }

            if let Some(max_days) = max_exception_window_days {
                let days_ahead = expires_date.signed_duration_since(today).num_days();
                if days_ahead > i64::from(max_days) {
                    expired.push(window_exceeded_exception(
                        allowlist_rel_path,
                        &e,
                        max_days,
                        days_ahead,
                    ));
                    continue;
                }
            }
        }

        entries.push(e);
    }

    let mut violations: Vec<Violation> = vec![];
    let mut suppressed: Vec<Violation> = vec![];

    violations.extend(expired);

    for v in input {
        if v.code.starts_with("exception.") {
            violations.push(v);
            continue;
        }

        let Some(path) = v.path.as_deref() else {
            violations.push(v);
            continue;
        };

        let path = normalize_exception_path(path);
        let matched = entries.iter().any(|e| e.rule == v.code && e.path == path);

        if matched {
            suppressed.push(v);
        } else {
            violations.push(v);
        }
    }

    SuppressionResult {
        violations,
        suppressed,
    }
}

pub fn apply_allowlist(repo_root: &Path, input: Vec<Violation>) -> SuppressionResult {
    apply_allowlist_with_limits(repo_root, input, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn v(code: &str, path: &str) -> Violation {
        Violation::blocking(code.to_string(), "m", Some(path.to_string()), None)
    }

    #[test]
    fn allowlist_suppresses_matching_violation() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();
        fs::write(
            repo_root.join(ALLOWLIST_REL_PATH),
            r#"
[[exceptions]]
id = "ex-1"
rule = "loc.max_exceeded"
path = "crates/x/lib.rs"
owner = "team"
reason = "temporary"
expires_at = "2999-01-01"
"#,
        )
        .unwrap();

        let r = apply_allowlist(repo_root, vec![v("loc.max_exceeded", "crates/x/lib.rs")]);
        assert!(r.violations.is_empty());
        assert_eq!(r.suppressed.len(), 1);
        assert_eq!(r.suppressed[0].code, "loc.max_exceeded");
    }

    #[test]
    fn allowlist_expired_exception_is_violation_and_does_not_suppress() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();
        fs::write(
            repo_root.join(ALLOWLIST_REL_PATH),
            r#"
[[exceptions]]
id = "ex-1"
rule = "loc.max_exceeded"
path = "crates/x/lib.rs"
owner = "team"
reason = "temporary"
expires_at = "2000-01-01"
"#,
        )
        .unwrap();

        let r = apply_allowlist(repo_root, vec![v("loc.max_exceeded", "crates/x/lib.rs")]);
        assert!(r.suppressed.is_empty());
        assert!(r.violations.iter().any(|v| v.code == "exception.expired"));
        assert!(r.violations.iter().any(|v| v.code == "loc.max_exceeded"));
    }

    #[test]
    fn allowlist_invalid_fails_closed() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();
        fs::write(
            repo_root.join(ALLOWLIST_REL_PATH),
            r#"
[[exceptions]]
id = "ex-1"
rule = "loc.max_exceeded"
path = "crates/*/lib.rs"
owner = "team"
reason = "bad"
expires_at = "2999-01-01"
"#,
        )
        .unwrap();

        let r = apply_allowlist(repo_root, vec![v("loc.max_exceeded", "crates/x/lib.rs")]);
        assert!(r.suppressed.is_empty());
        assert_eq!(r.violations[0].code, "exception.allowlist_invalid");
        assert!(r.violations.iter().any(|v| v.code == "loc.max_exceeded"));
    }

    #[test]
    fn allowlist_window_exceeded_is_violation_and_does_not_suppress() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();
        fs::write(
            repo_root.join(ALLOWLIST_REL_PATH),
            r#"
[[exceptions]]
id = "ex-1"
rule = "loc.max_exceeded"
path = "crates/x/lib.rs"
owner = "team"
reason = "temporary"
expires_at = "2999-01-01"
"#,
        )
        .unwrap();

        let r = apply_allowlist_with_limits(
            repo_root,
            vec![v("loc.max_exceeded", "crates/x/lib.rs")],
            Some(90),
        );
        assert!(r.suppressed.is_empty());
        assert!(
            r.violations
                .iter()
                .any(|v| v.code == "exception.window_exceeded")
        );
        assert!(r.violations.iter().any(|v| v.code == "loc.max_exceeded"));
    }
}
