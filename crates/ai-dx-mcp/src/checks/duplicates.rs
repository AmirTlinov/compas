use crate::api::Violation;
use crate::config::DuplicatesCheckConfigV2;
use crate::hash::sha256_hex;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicatesBaseline {
    pub groups: Vec<DuplicateGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub sha256: String,
    pub paths: Vec<String>,
}

#[derive(Debug)]
pub struct DuplicatesCheckResult {
    pub violations: Vec<Violation>,
    pub files_scanned: usize,
    pub files_universe: usize,
    pub groups_total: usize,
    pub duplicate_files_total: usize,
    pub groups: BTreeMap<String, Vec<String>>,
}

struct DuplicatesScan {
    groups: BTreeMap<String, Vec<String>>,
    files_scanned: usize,
    files_universe: usize,
    violations: Vec<Violation>,
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

fn scan_duplicate_groups(
    repo_root: &Path,
    cfg: &DuplicatesCheckConfigV2,
) -> Result<DuplicatesScan, String> {
    let include_globs = if cfg.include_globs.is_empty() {
        vec!["**/*".to_string()]
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

    let allowlist = if cfg.allowlist_globs.is_empty() {
        None
    } else {
        Some(build_globset(&cfg.allowlist_globs)?)
    };

    let mut rel_paths: Vec<String> = vec![];
    let mut files_universe = 0usize;
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
            Some(p) => p,
            None => continue,
        };
        if excludes.is_match(&rel) || !includes.is_match(&rel) {
            continue;
        }
        files_universe += 1;
        rel_paths.push(rel);
    }
    rel_paths.sort();

    let mut violations: Vec<Violation> = vec![];
    let mut by_hash: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut files_scanned = 0usize;

    for rel in rel_paths {
        let full = repo_root.join(Path::new(&rel));
        let meta = match fs::metadata(&full) {
            Ok(m) => m,
            Err(e) => {
                violations.push(Violation::blocking(
                    "duplicates.stat_failed",
                    format!("failed to stat file for duplicates scan: {e}"),
                    Some(rel),
                    None,
                ));
                continue;
            }
        };

        if meta.len() > cfg.max_file_bytes as u64 {
            continue;
        }

        let bytes = match fs::read(&full) {
            Ok(b) => b,
            Err(e) => {
                violations.push(Violation::blocking(
                    "duplicates.read_failed",
                    format!("failed to read file for duplicates scan: {e}"),
                    Some(rel),
                    None,
                ));
                continue;
            }
        };
        files_scanned += 1;
        let hash = sha256_hex(&bytes);
        by_hash.entry(hash).or_default().push(rel);
    }

    // Only keep groups that are truly duplicates (>=2 files) and are not fully allowlisted.
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (hash, mut paths) in by_hash {
        if paths.len() < 2 {
            continue;
        }
        paths.sort();

        let fully_allowlisted = allowlist
            .as_ref()
            .is_some_and(|g| paths.iter().all(|p| g.is_match(p)));
        if fully_allowlisted {
            continue;
        }

        groups.insert(hash, paths);
    }

    Ok(DuplicatesScan {
        groups,
        files_scanned,
        files_universe,
        violations,
    })
}

pub fn run_duplicates_check(
    repo_root: &Path,
    cfg: &DuplicatesCheckConfigV2,
) -> Result<DuplicatesCheckResult, String> {
    let scan = scan_duplicate_groups(repo_root, cfg)?;
    let current = scan.groups;
    let mut violations = scan.violations;

    if !current.is_empty() {
        let duplicate_files_total: usize = current.values().map(|v| v.len()).sum();
        violations.push(Violation::observation(
            "duplicates.found",
            format!(
                "duplicate files found (groups={}, files={})",
                current.len(),
                duplicate_files_total
            ),
            Some(cfg.baseline_path.clone()),
            Some(json!({
                "groups": current.len(),
                "files": duplicate_files_total,
                "examples": current.iter().take(5).map(|(sha, paths)| {
                    json!({
                        "sha256_prefix": sha.chars().take(12).collect::<String>(),
                        "paths": paths,
                    })
                }).collect::<Vec<_>>(),
            })),
        ));
    }

    let duplicate_files_total: usize = current.values().map(|v| v.len()).sum();
    Ok(DuplicatesCheckResult {
        violations,
        files_scanned: scan.files_scanned,
        files_universe: scan.files_universe,
        groups_total: current.len(),
        duplicate_files_total,
        groups: current,
    })
}
