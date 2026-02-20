use crate::api::Violation;
use crate::checks::common::{collect_candidate_files, is_probably_code_file};
use crate::config::ReuseFirstCheckConfigV2;
use crate::hash::sha256_hex;
use regex::Regex;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug)]
pub struct ReuseFirstCheckResult {
    pub scanned_blocks: usize,
    pub violations: Vec<Violation>,
}

#[derive(Debug, Clone)]
struct CodeBlock {
    rel_path: String,
    start_line: usize,
    symbol: String,
    normalized: String,
}

fn ext(rel: &str) -> Option<&str> {
    Path::new(rel).extension().and_then(|s| s.to_str())
}

fn strip_inline_comments(line: &str) -> &str {
    if let Some(idx) = line.find("//") {
        return &line[..idx];
    }
    if let Some(idx) = line.find('#') {
        return &line[..idx];
    }
    line
}

fn normalize_block(lines: &[String]) -> String {
    let mut joined = lines.join("\n").to_ascii_lowercase();
    for (pat, repl) in [
        (r"\bfn\s+[a-z_][a-z0-9_]*", "fn _"),
        (r"\bdef\s+[a-z_][a-z0-9_]*", "def _"),
        (r"\bfunction\s+[a-z_][a-z0-9_]*", "function _"),
        (r"\bfunc\s+[a-z_][a-z0-9_]*", "func _"),
    ] {
        if let Ok(re) = Regex::new(pat) {
            joined = re.replace_all(&joined, repl).to_string();
        }
    }
    joined
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .map(|l| strip_inline_comments(l))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
}

fn parse_symbol_from_line(line: &str) -> Option<String> {
    let patterns = [
        r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bfunc\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bdef\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bfunction\s+([A-Za-z_][A-Za-z0-9_]*)",
    ];
    for p in patterns {
        let re = Regex::new(p).ok()?;
        if let Some(c) = re.captures(line) {
            return Some(c.get(1)?.as_str().to_string());
        }
    }
    None
}

fn is_function_start(rel: &str, line: &str) -> bool {
    let t = line.trim_start();
    match ext(rel) {
        Some("rs") => {
            t.starts_with("fn ")
                || t.starts_with("pub fn ")
                || t.starts_with("pub(crate) fn ")
                || t.starts_with("pub async fn ")
        }
        Some("go") => t.starts_with("func "),
        Some("py") => t.starts_with("def "),
        Some("js") | Some("jsx") | Some("ts") | Some("tsx") => {
            t.starts_with("function ")
                || t.contains("=>")
                || t.starts_with("export function ")
                || t.starts_with("const ")
        }
        Some("c") | Some("h") | Some("cc") | Some("cpp") | Some("cxx") | Some("hpp")
        | Some("cs") => t.contains('(') && t.contains(')') && t.contains('{'),
        _ => false,
    }
}

fn extract_python_block(lines: &[String], start: usize) -> Vec<String> {
    let indent = lines[start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    let mut out = vec![lines[start].clone()];
    for line in &lines[start + 1..] {
        if line.trim().is_empty() {
            out.push(line.clone());
            continue;
        }
        let current_indent = line.chars().take_while(|c| c.is_whitespace()).count();
        if current_indent <= indent {
            break;
        }
        out.push(line.clone());
    }
    out
}

fn extract_brace_block(lines: &[String], start: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut balance: i32 = 0;
    let mut opened = false;
    for line in &lines[start..] {
        out.push(line.clone());
        for ch in line.chars() {
            if ch == '{' {
                opened = true;
                balance += 1;
            } else if ch == '}' {
                balance -= 1;
            }
        }
        if opened && balance <= 0 {
            break;
        }
    }
    out
}

fn extract_blocks_for_file(rel_path: &str, raw: &str, min_block_lines: usize) -> Vec<CodeBlock> {
    let lines: Vec<String> = raw.lines().map(ToString::to_string).collect();
    if lines.is_empty() {
        return vec![];
    }
    let mut blocks: Vec<CodeBlock> = vec![];
    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        if !is_function_start(rel_path, line) {
            i += 1;
            continue;
        }
        let symbol = parse_symbol_from_line(line).unwrap_or_else(|| format!("line_{}", i + 1));
        let block_lines = if matches!(ext(rel_path), Some("py")) {
            extract_python_block(&lines, i)
        } else {
            extract_brace_block(&lines, i)
        };
        let consumed = block_lines.len().max(1);
        if block_lines.len() >= min_block_lines {
            let normalized = normalize_block(&block_lines);
            if normalized.len() >= 32 {
                blocks.push(CodeBlock {
                    rel_path: rel_path.to_string(),
                    start_line: i + 1,
                    symbol,
                    normalized,
                });
            }
        }
        i += consumed;
    }
    blocks
}

pub fn run_reuse_first_check(
    repo_root: &Path,
    cfg: &ReuseFirstCheckConfigV2,
) -> ReuseFirstCheckResult {
    let mut violations = vec![];
    let mut blocks: Vec<CodeBlock> = vec![];

    let files = match collect_candidate_files(repo_root, &cfg.include_globs, &cfg.exclude_globs) {
        Ok(v) => v,
        Err(msg) => {
            return ReuseFirstCheckResult {
                scanned_blocks: 0,
                violations: vec![Violation::blocking(
                    "reuse_first.check_failed",
                    format!("reuse_first check failed (id={}): {msg}", cfg.id),
                    None,
                    None,
                )],
            };
        }
    };

    for (rel, path) in files {
        if !is_probably_code_file(&rel) {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "reuse_first.read_failed",
                    format!("failed to read {rel}: {e}"),
                    Some(rel.clone()),
                    None,
                ));
                continue;
            }
        };
        blocks.extend(extract_blocks_for_file(&rel, &raw, cfg.min_block_lines));
    }

    let mut by_hash: BTreeMap<String, Vec<&CodeBlock>> = BTreeMap::new();
    for b in &blocks {
        by_hash
            .entry(sha256_hex(b.normalized.as_bytes()))
            .or_default()
            .push(b);
    }

    for (fingerprint, group) in by_hash {
        if group.len() < 2 {
            continue;
        }
        let unique_paths: BTreeSet<&str> = group.iter().map(|b| b.rel_path.as_str()).collect();
        if unique_paths.len() < 2 {
            continue;
        }
        let symbols: Vec<String> = group
            .iter()
            .map(|b| format!("{}:{}:{}", b.rel_path, b.start_line, b.symbol))
            .collect();
        violations.push(Violation::blocking(
            "reuse_first.exact_duplicate",
            format!(
                "detected duplicate implementation blocks across {} files",
                unique_paths.len()
            ),
            None,
            Some(json!({
                "check_id": cfg.id,
                "fingerprint": fingerprint,
                "blocks": symbols,
            })),
        ));
    }

    ReuseFirstCheckResult {
        scanned_blocks: blocks.len(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_exact_duplicate_blocks() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/a.rs"),
            r#"
pub fn normalize(v: &str) -> String {
    let x = v.trim();
    if x.is_empty() { return String::new(); }
    x.to_ascii_lowercase()
}
"#,
        )
        .unwrap();
        std::fs::write(
            repo.join("src/b.rs"),
            r#"
pub fn normalize_copy(v: &str) -> String {
    let x = v.trim();
    if x.is_empty() { return String::new(); }
    x.to_ascii_lowercase()
}
"#,
        )
        .unwrap();
        let out = run_reuse_first_check(
            repo,
            &ReuseFirstCheckConfigV2 {
                id: "reuse".to_string(),
                include_globs: vec!["src/**/*.rs".to_string()],
                exclude_globs: vec![],
                min_block_lines: 3,
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "reuse_first.exact_duplicate")
        );
    }
}
