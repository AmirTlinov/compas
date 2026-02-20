use crate::api::Violation;
use crate::config::SurfaceCheckConfigV2;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceBaseline {
    pub items: Vec<String>,
}

#[derive(Debug)]
pub struct SurfaceCheckResult {
    pub violations: Vec<Violation>,
    pub files_scanned: usize,
    pub files_universe: usize,
    pub items_total: usize,
    pub max_items: usize,
    pub current_items: BTreeSet<String>,
}

struct SurfaceScan {
    items: BTreeSet<String>,
    files_scanned: usize,
    files_universe: usize,
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

struct CompiledRule {
    regex: Regex,
    desc: String,
    file_globs: Option<GlobSet>,
}

fn compile_rules(cfg: &SurfaceCheckConfigV2) -> Result<Vec<CompiledRule>, String> {
    if cfg.rules.is_empty() {
        return Err(format!(
            "surface rules are required (id={}); add checks.surface.rules",
            cfg.id
        ));
    }

    let mut out: Vec<CompiledRule> = vec![];
    for (idx, r) in cfg.rules.iter().enumerate() {
        let regex = Regex::new(&r.regex).map_err(|e| format!("bad surface rule regex: {e}"))?;
        let desc = r
            .description
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("rule{idx}"));

        let file_globs = if r.file_globs.is_empty() {
            None
        } else {
            Some(build_globset(&r.file_globs)?)
        };

        out.push(CompiledRule {
            regex,
            desc,
            file_globs,
        });
    }
    Ok(out)
}

fn scan_surface_items(repo_root: &Path, cfg: &SurfaceCheckConfigV2) -> Result<SurfaceScan, String> {
    let rules = compile_rules(cfg)?;

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

    let mut items: BTreeSet<String> = BTreeSet::new();
    let mut files_universe = 0usize;
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
        files_universe += 1;

        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {rel} for surface scan: {e}"))?;
        files_scanned += 1;

        let applicable: Vec<&CompiledRule> = rules
            .iter()
            .filter(|r| {
                r.file_globs
                    .as_ref()
                    .map(|g| g.is_match(&rel))
                    .unwrap_or(true)
            })
            .collect();
        if applicable.is_empty() {
            continue;
        }

        for line in source.lines() {
            let trimmed = line.trim_start();
            for rule in &applicable {
                let caps = match rule.regex.captures(trimmed) {
                    Some(c) => c,
                    None => continue,
                };
                let raw = caps
                    .get(1)
                    .or_else(|| caps.get(0))
                    .map(|m| m.as_str())
                    .unwrap_or_default();
                let val = raw.trim().trim_end_matches(';').trim();
                if val.is_empty() {
                    continue;
                }
                items.insert(format!("{rel}::{}:{val}", rule.desc));
            }
        }
    }

    Ok(SurfaceScan {
        items,
        files_scanned,
        files_universe,
    })
}

pub fn run_surface_check(
    repo_root: &Path,
    cfg: &SurfaceCheckConfigV2,
) -> Result<SurfaceCheckResult, String> {
    let scan = scan_surface_items(repo_root, cfg)?;
    let current = scan.items;

    let mut violations: Vec<Violation> = vec![];

    if current.len() > cfg.max_items {
        violations.push(Violation::observation(
            "surface.max_exceeded",
            format!(
                "surface exceeds max_items={} (current={})",
                cfg.max_items,
                current.len()
            ),
            Some(cfg.baseline_path.clone()),
            None,
        ));
    }

    Ok(SurfaceCheckResult {
        violations,
        files_scanned: scan.files_scanned,
        files_universe: scan.files_universe,
        items_total: current.len(),
        max_items: cfg.max_items,
        current_items: current,
    })
}
