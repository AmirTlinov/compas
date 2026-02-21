use crate::api::{FindingSeverity, Violation, ViolationTier};
use crate::hash::sha256_hex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolReportKind {
    #[default]
    Json,
    Sarif,
    Junit,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolReportSeverityMap {
    pub native: String,
    pub canonical: FindingSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolReportConfig {
    #[serde(default)]
    pub kind: ToolReportKind,
    pub path: String,
    pub expected_version: Option<String>,
    pub expected_sha256: Option<String>,
    pub commit_sha_field: Option<String>,
    pub adapter_id: Option<String>,
    #[serde(default)]
    pub severity_map: Vec<ToolReportSeverityMap>,
    #[serde(default)]
    pub default_category: Option<String>,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

impl Default for ToolReportConfig {
    fn default() -> Self {
        Self {
            kind: ToolReportKind::Json,
            path: "target/p22-reports/default.json".to_string(),
            expected_version: None,
            expected_sha256: None,
            commit_sha_field: None,
            adapter_id: None,
            severity_map: vec![],
            default_category: Some("general".to_string()),
            required: true,
        }
    }
}

#[derive(Debug)]
struct ParsedFinding {
    code: String,
    category: Option<String>,
    message: String,
    path: Option<String>,
    line: Option<u64>,
    severity_raw: String,
    evidence_ref: Option<String>,
}

#[derive(Debug)]
struct ParsedReport {
    findings: Vec<ParsedFinding>,
    version: Option<String>,
    commit_sha: Option<String>,
}

fn report_path(cfg: &ToolReportConfig, repo_root: &Path) -> PathBuf {
    let path = Path::new(&cfg.path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn violation(
    code: &str,
    message: impl Into<String>,
    path: Option<String>,
    details: Option<Value>,
) -> Violation {
    Violation {
        code: code.to_string(),
        message: message.into(),
        path,
        details,
        tier: ViolationTier::Blocking,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn u64_value(v: &Value) -> Option<u64> {
    if let Some(v) = v.as_u64() {
        return Some(v);
    }
    if let Some(v) = v.as_i64() {
        return (v >= 0).then_some(v as u64);
    }
    v.as_str().and_then(|s| s.parse::<u64>().ok())
}

fn find_json_path<'a>(root: &'a Value, dotted_path: &str) -> Option<&'a Value> {
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

fn message(v: &Value) -> String {
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

fn first_text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| v.get(*key).and_then(text))
}

fn parse_json_report(
    tool_id: &str,
    payload: &Value,
    cfg: &ToolReportConfig,
) -> Result<ParsedReport, String> {
    if !payload.is_object() {
        return Err(format!("tool={tool_id}: report root must be an object"));
    }

    let commit_sha = cfg
        .commit_sha_field
        .as_deref()
        .and_then(|field| find_json_path(payload, field))
        .and_then(text);

    let findings_values = payload
        .get("findings")
        .or_else(|| payload.get("results"))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("tool={tool_id}: missing findings/results array"))?;

    let mut findings = Vec::with_capacity(findings_values.len());
    for item in findings_values {
        let path = first_text(item, &["path", "file"])
            .or_else(|| item.get("location").and_then(text))
            .or_else(|| {
                item.get("location")
                    .and_then(|loc| loc.get("path"))
                    .and_then(text)
            });

        findings.push(ParsedFinding {
            code: first_text(
                item,
                &["code", "id", "rule_id", "ruleId", "name", "check_name"],
            )
            .unwrap_or_default(),
            category: first_text(item, &["category", "group", "family", "check_type"]),
            message: message(item),
            path,
            line: item
                .get("line")
                .or_else(|| item.get("start_line"))
                .or_else(|| item.get("startLine"))
                .and_then(u64_value),
            severity_raw: first_text(item, &["severity", "level", "priority", "impact"])
                .unwrap_or_else(|| "medium".to_string()),
            evidence_ref: first_text(item, &["evidence_ref", "url", "uri"]),
        });
    }

    if findings.is_empty() {
        return Err(format!("tool={tool_id}: report has no findings"));
    }

    Ok(ParsedReport {
        findings,
        version: payload.get("version").and_then(text),
        commit_sha,
    })
}

fn parse_sarif_report(tool_id: &str, payload: &Value) -> Result<ParsedReport, String> {
    let runs = payload
        .get("runs")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("tool={tool_id}: missing runs array"))?;

    let mut findings = Vec::new();
    for run in runs {
        let tool_name = run
            .get("tool")
            .and_then(|t| t.get("driver"))
            .and_then(|d| d.get("name"))
            .and_then(text);

        if let Some(results) = run.get("results").and_then(Value::as_array) {
            for result in results {
                let location = result
                    .get("locations")
                    .and_then(Value::as_array)
                    .and_then(|v| v.first());

                findings.push(ParsedFinding {
                    code: first_text(result, &["ruleId", "rule_id", "id"]).unwrap_or_default(),
                    category: first_text(result, &["category"]).or_else(|| tool_name.clone()),
                    message: message(result),
                    path: location
                        .and_then(|loc| loc.get("physicalLocation"))
                        .and_then(|pl| pl.get("artifactLocation"))
                        .and_then(|al| al.get("uri"))
                        .and_then(text),
                    line: location
                        .and_then(|loc| loc.get("physicalLocation"))
                        .and_then(|pl| pl.get("region"))
                        .and_then(|r| r.get("startLine"))
                        .and_then(u64_value),
                    severity_raw: first_text(result, &["level", "severity"])
                        .unwrap_or_else(|| "medium".to_string()),
                    evidence_ref: None,
                });
            }
        }
    }

    if findings.is_empty() {
        return Err(format!("tool={tool_id}: SARIF report has no findings"));
    }

    Ok(ParsedReport {
        findings,
        version: payload
            .get("version")
            .and_then(text)
            .or_else(|| payload.get("$schema").and_then(text)),
        commit_sha: None,
    })
}

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

fn parse_junit_report(tool_id: &str, input: &str) -> Result<ParsedReport, String> {
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
    })
}

fn parse_report(
    tool_id: &str,
    input: &str,
    cfg: &ToolReportConfig,
) -> Result<ParsedReport, String> {
    match cfg.kind {
        ToolReportKind::Junit => parse_junit_report(tool_id, input),
        ToolReportKind::Json => {
            let value: Value = serde_json::from_str(input)
                .map_err(|err| format!("tool={tool_id}: invalid JSON report: {err}"))?;
            parse_json_report(tool_id, &value, cfg)
        }
        ToolReportKind::Sarif => {
            let value: Value = serde_json::from_str(input)
                .map_err(|err| format!("tool={tool_id}: invalid SARIF report: {err}"))?;
            parse_sarif_report(tool_id, &value)
        }
        ToolReportKind::Auto => {
            let trimmed = input.trim_start();
            if trimmed.starts_with('<') {
                return parse_junit_report(tool_id, input);
            }
            let value: Value = serde_json::from_str(trimmed).map_err(|err| {
                format!("tool={tool_id}: failed to parse auto report as JSON: {err}")
            })?;
            if value.get("runs").is_some() {
                parse_sarif_report(tool_id, &value)
            } else {
                parse_json_report(tool_id, &value, cfg)
            }
        }
    }
}

fn current_head_sha(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

fn canonical_severity(
    tool_id: &str,
    cfg: &ToolReportConfig,
    raw: &str,
    path: Option<&str>,
) -> Result<FindingSeverity, Violation> {
    if let Some(map) = cfg
        .severity_map
        .iter()
        .find(|m| m.native.eq_ignore_ascii_case(raw))
    {
        return Ok(map.canonical);
    }

    match raw.trim().to_ascii_lowercase().as_str() {
        "critical" => Ok(FindingSeverity::Critical),
        "high" | "error" | "fatal" | "failure" => Ok(FindingSeverity::High),
        "medium" | "warning" | "warn" => Ok(FindingSeverity::Medium),
        "low" | "info" | "note" | "minor" => Ok(FindingSeverity::Low),
        _ => Err(violation(
            "tools.structured_report.invalid_severity",
            format!("tool={tool_id}: unknown severity {raw}"),
            path.map(str::to_string),
            Some(json!({ "raw_severity": raw })),
        )),
    }
}

fn severity_label(severity: FindingSeverity) -> &'static str {
    match severity {
        FindingSeverity::Critical => "critical",
        FindingSeverity::High => "high",
        FindingSeverity::Medium => "medium",
        FindingSeverity::Low => "low",
    }
}

fn finding_tier(severity: FindingSeverity) -> ViolationTier {
    if matches!(severity, FindingSeverity::Critical | FindingSeverity::High) {
        ViolationTier::Blocking
    } else {
        ViolationTier::Observation
    }
}

fn validate_version(
    tool_id: &str,
    cfg: &ToolReportConfig,
    parsed: &ParsedReport,
    violations: &mut Vec<Violation>,
) {
    if let Some(expected) = cfg.expected_version.as_deref() {
        match parsed.version.as_deref() {
            Some(actual) if expected == actual => {}
            Some(actual) => violations.push(violation(
                "tools.structured_report.version_mismatch",
                format!(
                    "tool={tool_id}: report version mismatch (expected={expected}, got={actual})"
                ),
                None,
                None,
            )),
            None => violations.push(violation(
                "tools.structured_report.version_missing",
                format!("tool={tool_id}: report version is missing"),
                None,
                None,
            )),
        }
    }
}

fn validate_commit(
    tool_id: &str,
    cfg: &ToolReportConfig,
    repo_root: &Path,
    parsed: &ParsedReport,
    violations: &mut Vec<Violation>,
) {
    let Some(field) = cfg.commit_sha_field.as_deref() else {
        return;
    };
    let Some(expected) = parsed.commit_sha.as_deref() else {
        violations.push(violation(
            "tools.structured_report.commit_field_missing",
            format!("tool={tool_id}: commit field `{field}` is missing"),
            None,
            None,
        ));
        return;
    };
    let Some(actual) = current_head_sha(repo_root) else {
        violations.push(violation(
            "tools.structured_report.commit_unavailable",
            format!("tool={tool_id}: unable to read repository HEAD"),
            None,
            None,
        ));
        return;
    };
    if actual != expected {
        violations.push(violation(
            "tools.structured_report.commit_mismatch",
            format!("tool={tool_id}: report commit mismatch"),
            None,
            Some(json!({"expected": expected, "actual": actual})),
        ));
    }
}

pub(crate) fn ingest_tool_report(
    repo_root: &Path,
    tool_id: &str,
    cfg_raw: &Value,
) -> (Option<Value>, Vec<Violation>) {
    let cfg: ToolReportConfig = match serde_json::from_value(cfg_raw.clone()) {
        Ok(cfg) => cfg,
        Err(err) => {
            return (
                None,
                vec![violation(
                    "tools.structured_report.invalid_config",
                    format!("tool={tool_id}: invalid report config: {err}"),
                    None,
                    None,
                )],
            );
        }
    };

    let report_path = report_path(&cfg, repo_root);
    if !report_path.exists() {
        if cfg.required {
            return (
                None,
                vec![violation(
                    "tools.structured_report.missing_report",
                    format!(
                        "tool={tool_id}: required report is missing: {}",
                        report_path.display()
                    ),
                    Some(report_path.display().to_string()),
                    None,
                )],
            );
        }
        return (None, vec![]);
    }

    let bytes = match std::fs::read(&report_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return (
                None,
                vec![violation(
                    "tools.structured_report.read_failed",
                    format!("tool={tool_id}: failed to read report: {err}"),
                    Some(report_path.display().to_string()),
                    None,
                )],
            );
        }
    };

    let report_sha = sha256_hex(&bytes);
    if let Some(expected) = cfg.expected_sha256.as_deref()
        && !expected.eq_ignore_ascii_case(&report_sha)
    {
        return (
            None,
            vec![violation(
                "tools.structured_report.sha256_mismatch",
                format!(
                    "tool={tool_id}: report sha256 mismatch (expected={expected}, got={report_sha})"
                ),
                Some(report_path.display().to_string()),
                None,
            )],
        );
    }

    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(err) => {
            return (
                None,
                vec![violation(
                    "tools.structured_report.invalid_encoding",
                    format!("tool={tool_id}: report is not utf-8: {err}"),
                    Some(report_path.display().to_string()),
                    None,
                )],
            );
        }
    };

    let mut parsed = match parse_report(tool_id, &text, &cfg) {
        Ok(parsed) => parsed,
        Err(err) => {
            return (
                None,
                vec![violation(
                    "tools.structured_report.parse_failed",
                    err,
                    Some(report_path.display().to_string()),
                    None,
                )],
            );
        }
    };

    let mut violations = vec![];
    validate_version(tool_id, &cfg, &parsed, &mut violations);
    validate_commit(tool_id, &cfg, repo_root, &parsed, &mut violations);

    let fallback_category = cfg
        .default_category
        .as_deref()
        .unwrap_or("general")
        .to_string();

    let mut findings_json = Vec::new();
    for finding in parsed.findings.drain(..) {
        if finding.code.trim().is_empty() {
            violations.push(violation(
                "tools.structured_report.invalid_finding_code",
                format!("tool={tool_id}: finding code is empty"),
                Some(report_path.display().to_string()),
                None,
            ));
            continue;
        }

        let category = finding
            .category
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_else(|| fallback_category.clone());

        let severity =
            match canonical_severity(tool_id, &cfg, &finding.severity_raw, Some(&finding.code)) {
                Ok(severity) => severity,
                Err(v) => {
                    violations.push(v);
                    continue;
                }
            };

        findings_json.push(json!({
            "code": finding.code,
            "severity": severity_label(severity),
            "category": category,
            "message": finding.message,
            "path": finding.path,
            "line": finding.line,
            "evidence_ref": finding.evidence_ref,
        }));

        violations.push(Violation {
            code: finding.code,
            message: format!(
                "tool={tool_id}: report finding severity={} category={}",
                severity_label(severity),
                category
            ),
            path: finding.path,
            details: Some(json!({
                "tool_id": tool_id,
                "line": finding.line,
                "severity": severity_label(severity),
                "category": category,
            })),
            tier: finding_tier(severity),
        });
    }

    let report = json!({
        "findings": findings_json,
        "evidence": {
            "report_path": report_path.display().to_string(),
            "report_sha256": report_sha,
            "report_version": parsed.version,
            "report_commit_sha": parsed.commit_sha,
            "adapter_id": cfg.adapter_id.clone(),
        }
    });

    (Some(report), violations)
}
