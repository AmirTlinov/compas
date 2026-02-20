use crate::api::Violation;
use crate::checks::common::{collect_candidate_files, is_probably_code_file};
use crate::config::ContractBreakCheckConfigV2;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug)]
pub struct ContractBreakCheckResult {
    pub symbols_total: usize,
    pub violations: Vec<Violation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContractSnapshot {
    version: u32,
    symbols: Vec<String>,
}

fn parse_public_symbols(rel: &str, raw: &str) -> Vec<String> {
    let mut out = vec![];
    let rust = Regex::new(
        r"^\s*pub\s+(?:async\s+)?(?:fn|struct|enum|trait|type|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .unwrap();
    let js_ts = Regex::new(
        r"^\s*export\s+(?:async\s+)?(?:function|class|const|let|var|type|interface)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .unwrap();
    let py = Regex::new(r"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();

    let ext = Path::new(rel)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    for line in raw.lines() {
        if let Some(c) = rust.captures(line) {
            out.push(c.get(1).unwrap().as_str().to_string());
            continue;
        }
        if let Some(c) = js_ts.captures(line) {
            out.push(c.get(1).unwrap().as_str().to_string());
            continue;
        }
        if ext == "py"
            && let Some(c) = py.captures(line)
        {
            let name = c.get(1).unwrap().as_str();
            if !name.starts_with('_') {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn load_snapshot(path: &Path) -> Result<ContractSnapshot, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read contract snapshot {:?}: {e}", path))?;
    serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse contract snapshot {:?}: {e}", path))
}

pub fn run_contract_break_check(
    repo_root: &Path,
    cfg: &ContractBreakCheckConfigV2,
) -> ContractBreakCheckResult {
    let mut violations = vec![];
    let files = match collect_candidate_files(repo_root, &cfg.include_globs, &cfg.exclude_globs) {
        Ok(v) => v,
        Err(msg) => {
            return ContractBreakCheckResult {
                symbols_total: 0,
                violations: vec![Violation::blocking(
                    "contract_break.check_failed",
                    format!("contract_break check failed (id={}): {msg}", cfg.id),
                    None,
                    None,
                )],
            };
        }
    };

    let mut current: BTreeSet<String> = BTreeSet::new();
    for (rel, path) in files {
        if !is_probably_code_file(&rel) {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "contract_break.read_failed",
                    format!("failed to read {rel}: {e}"),
                    Some(rel.clone()),
                    None,
                ));
                continue;
            }
        };
        for s in parse_public_symbols(&rel, &raw) {
            current.insert(s);
        }
    }

    let snapshot_path = repo_root.join(&cfg.baseline_path);
    let baseline = if snapshot_path.is_file() {
        match load_snapshot(&snapshot_path) {
            Ok(v) => Some(v),
            Err(msg) => {
                violations.push(Violation::blocking(
                    "contract_break.baseline_invalid",
                    msg,
                    Some(cfg.baseline_path.clone()),
                    None,
                ));
                None
            }
        }
    } else {
        violations.push(Violation::blocking(
            "contract_break.baseline_missing",
            "contract baseline is missing; create baseline before enabling strict compatibility gate",
            Some(cfg.baseline_path.clone()),
            Some(json!({ "check_id": cfg.id })),
        ));
        None
    };

    if let Some(base) = baseline {
        let base_set: BTreeSet<String> = base.symbols.into_iter().collect();
        for removed in base_set.difference(&current) {
            violations.push(Violation::blocking(
                "contract_break.removed_symbol",
                format!(
                    "breaking change detected: removed public symbol {}",
                    removed
                ),
                Some(cfg.baseline_path.clone()),
                Some(json!({ "check_id": cfg.id, "symbol": removed })),
            ));
        }
        if !cfg.allow_additions {
            for added in current.difference(&base_set) {
                violations.push(Violation::blocking(
                    "contract_break.added_symbol",
                    format!("public contract addition not allowed by policy: {}", added),
                    Some(cfg.baseline_path.clone()),
                    Some(json!({ "check_id": cfg.id, "symbol": added })),
                ));
            }
        }
    }

    ContractBreakCheckResult {
        symbols_total: current.len(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_removed_symbol_vs_baseline() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join(".agents/mcp/compas/baselines")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn still_here() {}\n").unwrap();
        std::fs::write(
            repo.join(".agents/mcp/compas/baselines/contracts.json"),
            serde_json::to_string_pretty(&ContractSnapshot {
                version: 1,
                symbols: vec!["still_here".to_string(), "removed_api".to_string()],
            })
            .unwrap(),
        )
        .unwrap();
        let out = run_contract_break_check(
            repo,
            &ContractBreakCheckConfigV2 {
                id: "contract".to_string(),
                include_globs: vec!["src/**/*.rs".to_string()],
                exclude_globs: vec![],
                baseline_path: ".agents/mcp/compas/baselines/contracts.json".to_string(),
                allow_additions: true,
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "contract_break.removed_symbol")
        );
    }
}
