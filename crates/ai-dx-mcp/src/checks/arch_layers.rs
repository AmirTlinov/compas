use crate::api::Violation;
use crate::checks::common::{collect_candidate_files, is_probably_code_file};
use crate::config::{ArchLayerConfigV2, ArchLayersCheckConfigV2};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug)]
pub struct ArchLayersCheckResult {
    pub edges_total: usize,
    pub violations: Vec<Violation>,
}

fn build_globset(globs: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in globs {
        let g = Glob::new(p).map_err(|e| format!("invalid arch layer glob {:?}: {e}", p))?;
        b.add(g);
    }
    b.build()
        .map_err(|e| format!("failed to build arch layer globset: {e}"))
}

fn layer_of_path<'a>(layers: &'a [ArchLayerConfigV2], rel_path: &str) -> Option<&'a str> {
    for layer in layers {
        if layer.include_globs.is_empty() {
            continue;
        }
        if let Ok(gs) = build_globset(&layer.include_globs)
            && gs.is_match(rel_path)
        {
            return Some(layer.id.as_str());
        }
    }
    None
}

fn import_tokens(line: &str) -> Vec<String> {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix("use crate::")
        && let Some(first) = rest.split("::").next()
        && !first.is_empty()
    {
        return vec![first.trim().to_string()];
    }
    if let Some(rest) = t.strip_prefix("from ")
        && let Some(first) = rest.split('.').next()
    {
        return vec![first.trim().to_string()];
    }
    if let Some(rest) = t.strip_prefix("import ") {
        return rest
            .split(',')
            .filter_map(|part| part.trim().split('.').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if t.starts_with("import ")
        && t.contains(" from ")
        && let Some((_lhs, rhs)) = t.split_once(" from ")
    {
        let clean = rhs
            .trim()
            .trim_matches(';')
            .trim_matches('"')
            .trim_matches('\'');
        if let Some(first) = clean.split('/').next()
            && !first.starts_with('.')
            && !first.is_empty()
        {
            return vec![first.to_string()];
        }
    }
    if let Some((_lhs, rhs)) = t.split_once("require(") {
        let clean = rhs
            .trim()
            .trim_start_matches('"')
            .trim_start_matches('\'')
            .split(['"', '\'', ')'])
            .next()
            .unwrap_or_default();
        if let Some(first) = clean.split('/').next()
            && !first.starts_with('.')
            && !first.is_empty()
        {
            return vec![first.to_string()];
        }
    }
    vec![]
}

fn layer_for_token<'a>(layers: &'a [ArchLayerConfigV2], token: &str) -> Option<&'a str> {
    for layer in layers {
        if layer
            .module_prefixes
            .iter()
            .any(|p| p.eq_ignore_ascii_case(token))
        {
            return Some(layer.id.as_str());
        }
    }
    None
}

fn has_cycle(edges: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Temp,
        Perm,
    }
    fn dfs(
        node: &str,
        edges: &BTreeMap<String, BTreeSet<String>>,
        marks: &mut BTreeMap<String, Mark>,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        marks.insert(node.to_string(), Mark::Temp);
        stack.push(node.to_string());
        if let Some(nexts) = edges.get(node) {
            for n in nexts {
                match marks.get(n.as_str()) {
                    Some(Mark::Temp) => {
                        let idx = stack.iter().position(|x| x == n).unwrap_or(0);
                        return Some(stack[idx..].to_vec());
                    }
                    Some(Mark::Perm) => {}
                    None => {
                        if let Some(c) = dfs(n, edges, marks, stack) {
                            return Some(c);
                        }
                    }
                }
            }
        }
        stack.pop();
        marks.insert(node.to_string(), Mark::Perm);
        None
    }
    let mut marks: BTreeMap<String, Mark> = BTreeMap::new();
    let mut stack = vec![];
    for node in edges.keys() {
        if !marks.contains_key(node)
            && let Some(c) = dfs(node, edges, &mut marks, &mut stack)
        {
            return Some(c);
        }
    }
    None
}

pub fn run_arch_layers_check(
    repo_root: &Path,
    cfg: &ArchLayersCheckConfigV2,
) -> ArchLayersCheckResult {
    if cfg.layers.is_empty() {
        return ArchLayersCheckResult {
            edges_total: 0,
            violations: vec![Violation::blocking(
                "arch_layers.invalid_config",
                format!("arch_layers check id={} has no layers", cfg.id),
                None,
                None,
            )],
        };
    }

    let mut violations = vec![];
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut scanned_edges = 0usize;

    let files = match collect_candidate_files(repo_root, &["**/*".to_string()], &[]) {
        Ok(v) => v,
        Err(msg) => {
            return ArchLayersCheckResult {
                edges_total: 0,
                violations: vec![Violation::blocking(
                    "arch_layers.check_failed",
                    format!("arch_layers check failed (id={}): {msg}", cfg.id),
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
        let Some(src_layer) = layer_of_path(&cfg.layers, &rel) else {
            continue;
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "arch_layers.read_failed",
                    format!("failed to read {rel}: {e}"),
                    Some(rel.clone()),
                    None,
                ));
                continue;
            }
        };
        for line in raw.lines() {
            for token in import_tokens(line) {
                if let Some(dst_layer) = layer_for_token(&cfg.layers, &token) {
                    if dst_layer == src_layer {
                        continue;
                    }
                    edges
                        .entry(src_layer.to_string())
                        .or_default()
                        .insert(dst_layer.to_string());
                    scanned_edges += 1;
                }
            }
        }
    }

    for rule in &cfg.rules {
        let deny: BTreeSet<&str> = rule.deny_to_layers.iter().map(|s| s.as_str()).collect();
        if deny.is_empty() {
            continue;
        }
        if let Some(outgoing) = edges.get(rule.from_layer.as_str()) {
            for d in outgoing {
                if deny.contains(d.as_str()) {
                    violations.push(Violation::blocking(
                        "arch_layers.rule_violation",
                        format!(
                            "forbidden layer edge detected: {} -> {}",
                            rule.from_layer, d
                        ),
                        None,
                        Some(json!({
                            "check_id": cfg.id,
                            "from_layer": rule.from_layer,
                            "to_layer": d,
                        })),
                    ));
                }
            }
        }
    }

    if let Some(cycle) = has_cycle(&edges)
        && cycle.len() > 1
    {
        violations.push(Violation::blocking(
            "arch_layers.cycle_detected",
            "detected cycle in architecture layer graph",
            None,
            Some(json!({
                "check_id": cfg.id,
                "cycle": cycle,
            })),
        ));
    }

    ArchLayersCheckResult {
        edges_total: scanned_edges,
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_forbidden_edge() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src/app")).unwrap();
        std::fs::create_dir_all(repo.join("src/infra")).unwrap();
        std::fs::write(repo.join("src/app/mod.rs"), "use crate::infra::db::Repo;").unwrap();
        std::fs::write(repo.join("src/infra/db.rs"), "pub struct Repo;").unwrap();

        let out = run_arch_layers_check(
            repo,
            &ArchLayersCheckConfigV2 {
                id: "layers".to_string(),
                layers: vec![
                    ArchLayerConfigV2 {
                        id: "app".to_string(),
                        include_globs: vec!["src/app/**".to_string()],
                        module_prefixes: vec!["app".to_string()],
                    },
                    ArchLayerConfigV2 {
                        id: "infra".to_string(),
                        include_globs: vec!["src/infra/**".to_string()],
                        module_prefixes: vec!["infra".to_string()],
                    },
                ],
                rules: vec![crate::config::ArchLayerRuleConfigV2 {
                    from_layer: "app".to_string(),
                    deny_to_layers: vec!["infra".to_string()],
                }],
            },
        );
        assert!(
            out.violations
                .iter()
                .any(|v| v.code == "arch_layers.rule_violation")
        );
    }
}
