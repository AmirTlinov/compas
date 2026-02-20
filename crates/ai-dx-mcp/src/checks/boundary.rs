use crate::api::Violation;
use crate::config::BoundaryCheckConfigV2;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde_json::json;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct BoundaryCheckResult {
    pub violations: Vec<Violation>,
    pub files_scanned: usize,
    pub rules_checked: usize,
}

fn build_globset(globs: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for g in globs {
        let glob = Glob::new(g).map_err(|e| format!("bad glob {g:?}: {e}"))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| format!("failed to build globset: {e}"))
}

fn normalize_rel_path(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn line_for_offset(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

fn brace_delta(line: &str) -> isize {
    let opens = line.chars().filter(|c| *c == '{').count() as isize;
    let closes = line.chars().filter(|c| *c == '}').count() as isize;
    opens - closes
}

fn looks_like_rust_mod_decl_with_body(line: &str) -> bool {
    let t = line.trim_start();
    let starts_like_mod = t.starts_with("mod ")
        || t.starts_with("pub mod ")
        || t.starts_with("pub(crate) mod ")
        || t.starts_with("pub(super) mod ")
        || t.starts_with("pub(in ");
    starts_like_mod && t.contains('{')
}

fn strip_rust_cfg_test_modules(source: &str) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            // Preserve line numbering for diagnostics.
            if i + 1 < lines.len() {
                out.push('\n');
            }
            i += 1;

            // Optional extra attributes before the test module declaration.
            while i < lines.len() && lines[i].trim_start().starts_with("#[") {
                if i + 1 < lines.len() {
                    out.push('\n');
                }
                i += 1;
            }

            if i < lines.len() && looks_like_rust_mod_decl_with_body(lines[i]) {
                let mut depth = brace_delta(lines[i]);
                if i + 1 < lines.len() {
                    out.push('\n');
                }
                i += 1;
                while i < lines.len() && depth > 0 {
                    depth += brace_delta(lines[i]);
                    if i + 1 < lines.len() {
                        out.push('\n');
                    }
                    i += 1;
                }
                continue;
            }

            continue;
        }

        out.push_str(lines[i]);
        if i + 1 < lines.len() {
            out.push('\n');
        }
        i += 1;
    }

    out
}

pub fn run_boundary_check(
    repo_root: &Path,
    cfg: &BoundaryCheckConfigV2,
) -> Result<BoundaryCheckResult, String> {
    let include_globs = if cfg.include_globs.is_empty() {
        vec!["crates/**/*.rs".to_string()]
    } else {
        cfg.include_globs.clone()
    };
    let exclude_globs = if cfg.exclude_globs.is_empty() {
        vec!["**/target/**".to_string(), ".git/**".to_string()]
    } else {
        cfg.exclude_globs.clone()
    };

    let includes = build_globset(&include_globs)?;
    let excludes = build_globset(&exclude_globs)?;

    let mut compiled_rules: Vec<(String, String, Regex)> = vec![];
    for rule in &cfg.rules {
        let id = rule.id.trim();
        if id.is_empty() {
            return Err("boundary rule has empty id".to_string());
        }
        let regex = Regex::new(rule.deny_regex.trim()).map_err(|e| {
            format!(
                "failed to compile boundary rule regex id={id} regex={:?}: {e}",
                rule.deny_regex
            )
        })?;
        let message = rule
            .message
            .clone()
            .unwrap_or_else(|| "boundary rule violation".to_string());
        compiled_rules.push((id.to_string(), message, regex));
    }

    let mut violations: Vec<Violation> = vec![];
    let mut files_scanned = 0usize;

    for entry in WalkDir::new(repo_root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = match normalize_rel_path(repo_root, path) {
            Some(v) => v,
            None => continue,
        };
        if excludes.is_match(&rel) || !includes.is_match(&rel) {
            continue;
        }

        files_scanned += 1;

        let source = match std::fs::read_to_string(path) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "boundary.read_failed",
                    format!("failed to read file for boundary check: {e}"),
                    Some(rel),
                    None,
                ));
                continue;
            }
        };

        let source_for_scan = if cfg.strip_rust_cfg_test_blocks && rel.ends_with(".rs") {
            strip_rust_cfg_test_modules(&source)
        } else {
            source
        };

        for (rule_id, rule_message, regex) in &compiled_rules {
            if let Some(m) = regex.find(&source_for_scan) {
                let line = line_for_offset(&source_for_scan, m.start());
                violations.push(Violation::blocking(
                    "boundary.rule_violation",
                    format!("{rule_message} (rule_id={rule_id})"),
                    Some(rel.clone()),
                    Some(json!({
                        "rule_id": rule_id,
                        "line": line,
                        "matched": m.as_str(),
                    })),
                ));
            }
        }
    }

    Ok(BoundaryCheckResult {
        violations,
        files_scanned,
        rules_checked: compiled_rules.len(),
    })
}
