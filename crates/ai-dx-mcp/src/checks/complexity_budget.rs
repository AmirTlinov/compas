use crate::api::Violation;
use crate::checks::common::{collect_candidate_files, is_probably_code_file};
use crate::config::ComplexityBudgetCheckConfigV2;
use regex::Regex;
use serde_json::json;
use std::path::Path;

#[derive(Debug)]
pub struct ComplexityBudgetCheckResult {
    pub scanned_functions: usize,
    pub violations: Vec<Violation>,
}

#[derive(Debug, Clone)]
struct FnBlock {
    rel_path: String,
    start_line: usize,
    symbol: String,
    lines: Vec<String>,
}

fn ext(rel: &str) -> Option<&str> {
    Path::new(rel).extension().and_then(|s| s.to_str())
}

fn parse_symbol(line: &str) -> String {
    let patterns = [
        r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bfunc\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bdef\s+([A-Za-z_][A-Za-z0-9_]*)",
        r"\bfunction\s+([A-Za-z_][A-Za-z0-9_]*)",
    ];
    for p in patterns {
        if let Ok(re) = Regex::new(p)
            && let Some(c) = re.captures(line)
            && let Some(m) = c.get(1)
        {
            return m.as_str().to_string();
        }
    }
    "anonymous".to_string()
}

fn is_fn_start(rel: &str, line: &str) -> bool {
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
                || t.starts_with("export function ")
                || (t.starts_with("const ") && t.contains("=>"))
        }
        Some("c") | Some("h") | Some("cc") | Some("cpp") | Some("cxx") | Some("hpp")
        | Some("cs") => t.contains('(') && t.contains(')') && t.contains('{'),
        _ => false,
    }
}

fn extract_python(lines: &[String], start: usize) -> Vec<String> {
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
        let current = line.chars().take_while(|c| c.is_whitespace()).count();
        if current <= indent {
            break;
        }
        out.push(line.clone());
    }
    out
}

fn extract_braces(lines: &[String], start: usize) -> Vec<String> {
    let mut out = vec![];
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

fn extract_functions(rel: &str, raw: &str) -> Vec<FnBlock> {
    let lines: Vec<String> = raw.lines().map(ToString::to_string).collect();
    let mut out: Vec<FnBlock> = vec![];
    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        if !is_fn_start(rel, line) {
            i += 1;
            continue;
        }
        let block_lines = if matches!(ext(rel), Some("py")) {
            extract_python(&lines, i)
        } else {
            extract_braces(&lines, i)
        };
        let consumed = block_lines.len().max(1);
        out.push(FnBlock {
            rel_path: rel.to_string(),
            start_line: i + 1,
            symbol: parse_symbol(line),
            lines: block_lines,
        });
        i += consumed;
    }
    out
}

fn cyclomatic(lines: &[String]) -> usize {
    let mut count = 1usize;
    for line in lines {
        let t = line.trim();
        for kw in [" if ", " else if ", " for ", " while ", " match ", " case "] {
            if format!(" {t} ").contains(kw) {
                count += 1;
            }
        }
        count += t.matches("&&").count();
        count += t.matches("||").count();
        count += t.matches('?').count();
    }
    count
}

fn cognitive(lines: &[String], py: bool) -> usize {
    let mut score = 0usize;
    let mut depth = 0usize;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !py {
            depth += line.matches('{').count();
        }
        if ["if ", "for ", "while ", "match ", "else if ", "case "]
            .iter()
            .any(|kw| trimmed.starts_with(kw) || trimmed.contains(&format!(" {kw}")))
        {
            score += 1 + depth;
        }
        if !py {
            depth = depth.saturating_sub(line.matches('}').count());
        } else {
            let indent = line.chars().take_while(|c| c.is_whitespace()).count() / 4;
            depth = indent;
        }
    }
    score.max(1)
}

pub fn run_complexity_budget_check(
    repo_root: &Path,
    cfg: &ComplexityBudgetCheckConfigV2,
) -> ComplexityBudgetCheckResult {
    let mut violations = vec![];
    let mut all_fns: Vec<FnBlock> = vec![];
    let files = match collect_candidate_files(repo_root, &cfg.include_globs, &cfg.exclude_globs) {
        Ok(v) => v,
        Err(msg) => {
            return ComplexityBudgetCheckResult {
                scanned_functions: 0,
                violations: vec![Violation::blocking(
                    "complexity_budget.check_failed",
                    format!("complexity_budget check failed (id={}): {msg}", cfg.id),
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
                    "complexity_budget.read_failed",
                    format!("failed to read {rel}: {e}"),
                    Some(rel.clone()),
                    None,
                ));
                continue;
            }
        };
        all_fns.extend(extract_functions(&rel, &raw));
    }

    for f in &all_fns {
        let line_count = f.lines.len();
        let cyc = cyclomatic(&f.lines);
        let cog = cognitive(&f.lines, matches!(ext(&f.rel_path), Some("py")));
        if line_count > cfg.max_function_lines
            || cyc > cfg.max_cyclomatic
            || cog > cfg.max_cognitive
        {
            violations.push(Violation::blocking(
                "complexity_budget.threshold_exceeded",
                format!(
                    "function {} exceeds complexity budget (lines={}, cyclomatic={}, cognitive={})",
                    f.symbol, line_count, cyc, cog
                ),
                Some(f.rel_path.clone()),
                Some(json!({
                    "check_id": cfg.id,
                    "symbol": f.symbol,
                    "start_line": f.start_line,
                    "line_count": line_count,
                    "cyclomatic": cyc,
                    "cognitive": cog,
                    "limits": {
                        "max_function_lines": cfg.max_function_lines,
                        "max_cyclomatic": cfg.max_cyclomatic,
                        "max_cognitive": cfg.max_cognitive,
                    }
                })),
            ));
        }
    }

    ComplexityBudgetCheckResult {
        scanned_functions: all_fns.len(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_over_complex_function() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/lib.rs"),
            r#"
pub fn big(x: i32) -> i32 {
    if x > 0 { if x > 1 { if x > 2 { if x > 3 { return x; }}}}
    for _i in 0..10 { if x > 5 { return x; } }
    x
}
"#,
        )
        .unwrap();
        let out = run_complexity_budget_check(
            repo,
            &ComplexityBudgetCheckConfigV2 {
                id: "cx".to_string(),
                include_globs: vec!["src/**/*.rs".to_string()],
                exclude_globs: vec![],
                max_function_lines: 3,
                max_cyclomatic: 2,
                max_cognitive: 2,
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "complexity_budget.threshold_exceeded")
        );
    }
}
