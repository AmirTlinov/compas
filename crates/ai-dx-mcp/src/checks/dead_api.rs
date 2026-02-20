use crate::api::Violation;
use crate::checks::common::{collect_candidate_files, is_probably_code_file};
use crate::config::{DeadCodeCheckConfigV2, OrphanApiCheckConfigV2};
use regex::Regex;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug)]
struct Symbol {
    name: String,
    rel_path: String,
    line: usize,
    public: bool,
}

#[derive(Debug)]
pub struct DeadCodeCheckResult {
    pub symbols_scanned: usize,
    pub violations: Vec<Violation>,
}

#[derive(Debug)]
pub struct OrphanApiCheckResult {
    pub symbols_scanned: usize,
    pub violations: Vec<Violation>,
}

fn parse_symbols(rel: &str, raw: &str) -> Vec<Symbol> {
    let mut out = vec![];
    let re_rust = Regex::new(r"^\s*(pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let re_rust_type =
        Regex::new(r"^\s*pub\s+(?:struct|enum|trait|mod|type)\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let re_py = Regex::new(r"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();
    let re_js_fn = Regex::new(r"^\s*(export\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();
    let re_js_const = Regex::new(
        r"^\s*(export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?\(",
    )
    .unwrap();

    for (idx, line) in raw.lines().enumerate() {
        if let Some(c) = re_rust.captures(line) {
            out.push(Symbol {
                name: c.get(2).unwrap().as_str().to_string(),
                rel_path: rel.to_string(),
                line: idx + 1,
                public: c.get(1).is_some(),
            });
            continue;
        }
        if let Some(c) = re_rust_type.captures(line) {
            out.push(Symbol {
                name: c.get(1).unwrap().as_str().to_string(),
                rel_path: rel.to_string(),
                line: idx + 1,
                public: true,
            });
            continue;
        }
        if let Some(c) = re_py.captures(line) {
            let name = c.get(1).unwrap().as_str().to_string();
            out.push(Symbol {
                public: false,
                name,
                rel_path: rel.to_string(),
                line: idx + 1,
            });
            continue;
        }
        if let Some(c) = re_js_fn.captures(line) {
            out.push(Symbol {
                name: c.get(2).unwrap().as_str().to_string(),
                rel_path: rel.to_string(),
                line: idx + 1,
                public: c.get(1).is_some(),
            });
            continue;
        }
        if let Some(c) = re_js_const.captures(line) {
            out.push(Symbol {
                name: c.get(2).unwrap().as_str().to_string(),
                rel_path: rel.to_string(),
                line: idx + 1,
                public: c.get(1).is_some(),
            });
        }
    }
    out
}

fn symbol_usage_counts(texts: &[(String, String)], names: &[String]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for n in names {
        if n.is_empty() {
            continue;
        }
        let pat = Regex::new(&format!(r"\b{}\b", regex::escape(n))).unwrap();
        let c = texts
            .iter()
            .map(|(_, t)| pat.find_iter(t).count())
            .sum::<usize>();
        counts.insert(n.clone(), c);
    }
    counts
}

#[allow(clippy::type_complexity)]
fn collect_symbols(
    repo_root: &Path,
    include_globs: &[String],
    exclude_globs: &[String],
) -> Result<(Vec<Symbol>, Vec<(String, String)>), String> {
    let mut symbols = vec![];
    let mut texts = vec![];
    let files = collect_candidate_files(repo_root, include_globs, exclude_globs)?;
    for (rel, path) in files {
        if !is_probably_code_file(&rel) {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read code file {rel}: {e}"))?;
        symbols.extend(parse_symbols(&rel, &raw));
        texts.push((rel, raw));
    }
    Ok((symbols, texts))
}

pub fn run_dead_code_check(repo_root: &Path, cfg: &DeadCodeCheckConfigV2) -> DeadCodeCheckResult {
    let (symbols, texts) = match collect_symbols(repo_root, &cfg.include_globs, &cfg.exclude_globs)
    {
        Ok(v) => v,
        Err(msg) => {
            return DeadCodeCheckResult {
                symbols_scanned: 0,
                violations: vec![Violation::blocking(
                    "dead_code.check_failed",
                    format!("dead_code check failed (id={}): {msg}", cfg.id),
                    None,
                    None,
                )],
            };
        }
    };

    let private: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| !s.public && s.name.len() >= cfg.min_symbol_len)
        .collect();
    let names: Vec<String> = private.iter().map(|s| s.name.clone()).collect();
    let counts = symbol_usage_counts(&texts, &names);

    let mut violations = vec![];
    for s in private {
        let usage = counts.get(&s.name).copied().unwrap_or(0);
        if usage <= 1 {
            let mk = if cfg.blocking {
                Violation::blocking
            } else {
                Violation::observation
            };
            violations.push(mk(
                "dead_code.unused_symbol",
                format!("private symbol appears unused: {}", s.name),
                Some(s.rel_path.clone()),
                Some(json!({
                    "check_id": cfg.id,
                    "symbol": s.name,
                    "line": s.line,
                    "usage_count": usage,
                })),
            ));
        }
    }

    DeadCodeCheckResult {
        symbols_scanned: symbols.len(),
        violations,
    }
}

pub fn run_orphan_api_check(
    repo_root: &Path,
    cfg: &OrphanApiCheckConfigV2,
) -> OrphanApiCheckResult {
    let (symbols, texts) = match collect_symbols(repo_root, &cfg.include_globs, &cfg.exclude_globs)
    {
        Ok(v) => v,
        Err(msg) => {
            return OrphanApiCheckResult {
                symbols_scanned: 0,
                violations: vec![Violation::blocking(
                    "orphan_api.check_failed",
                    format!("orphan_api check failed (id={}): {msg}", cfg.id),
                    None,
                    None,
                )],
            };
        }
    };

    let public_symbols: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| s.public && s.name.len() >= cfg.min_symbol_len)
        .collect();
    let names: Vec<String> = public_symbols.iter().map(|s| s.name.clone()).collect();
    let counts = symbol_usage_counts(&texts, &names);

    let mut violations = vec![];
    for s in public_symbols {
        let usage = counts.get(&s.name).copied().unwrap_or(0);
        if usage <= 1 {
            let mk = if cfg.blocking {
                Violation::blocking
            } else {
                Violation::observation
            };
            violations.push(mk(
                "orphan_api.unused_public_symbol",
                format!("public symbol appears orphaned: {}", s.name),
                Some(s.rel_path.clone()),
                Some(json!({
                    "check_id": cfg.id,
                    "symbol": s.name,
                    "line": s.line,
                    "usage_count": usage,
                })),
            ));
        }
    }

    OrphanApiCheckResult {
        symbols_scanned: symbols.len(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn dead_code_detects_private_unused() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/lib.rs"),
            r#"
fn helper_unused() -> i32 { 1 }
pub fn api() -> i32 { 2 }
"#,
        )
        .unwrap();
        let out = run_dead_code_check(
            repo,
            &DeadCodeCheckConfigV2 {
                id: "dead".to_string(),
                include_globs: vec!["src/**/*.rs".to_string()],
                exclude_globs: vec![],
                min_symbol_len: 3,
                blocking: false,
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "dead_code.unused_symbol")
        );
    }

    #[test]
    fn orphan_api_detects_public_unused() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src/lib.rs"),
            r#"
pub fn api_orphan() -> i32 { 1 }
"#,
        )
        .unwrap();
        let out = run_orphan_api_check(
            repo,
            &OrphanApiCheckConfigV2 {
                id: "orphan".to_string(),
                include_globs: vec!["src/**/*.rs".to_string()],
                exclude_globs: vec![],
                min_symbol_len: 3,
                blocking: false,
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "orphan_api.unused_public_symbol")
        );
    }
}
