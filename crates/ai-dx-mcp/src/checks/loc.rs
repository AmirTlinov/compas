use crate::api::Violation;
use crate::config::LocCheckConfigV2;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocBaseline {
    pub files: BTreeMap<String, usize>,
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

fn count_non_empty_lines(bytes: &[u8]) -> usize {
    let mut count = 0usize;
    for line in bytes.split(|b| *b == b'\n') {
        let mut s = line;
        while let Some((first, rest)) = s.split_first() {
            if matches!(first, b' ' | b'\t' | b'\r') {
                s = rest;
                continue;
            }
            break;
        }
        if !s.is_empty() {
            count += 1;
        }
    }
    count
}

pub struct LocCheckResult {
    pub violations: Vec<Violation>,
    pub files_scanned: usize,
    pub files_universe: usize,
    pub max_loc: usize,
    pub worst_path: Option<String>,
    pub loc_per_file: BTreeMap<String, usize>,
}

pub fn run_loc_check(repo_root: &Path, cfg: &LocCheckConfigV2) -> Result<LocCheckResult, String> {
    let include_globs = if cfg.include_globs.is_empty() {
        vec!["**/*.rs".to_string()]
    } else {
        cfg.include_globs.clone()
    };
    let exclude_globs = cfg.exclude_globs.clone();

    let includes = build_globset(&include_globs)?;
    let excludes = build_globset(&exclude_globs)?;

    let mut files: BTreeMap<String, usize> = BTreeMap::new();
    let mut violations: Vec<Violation> = vec![];
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

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                violations.push(Violation::blocking(
                    "loc.read_failed",
                    format!("failed to read file for LOC scan: {e}"),
                    Some(rel),
                    None,
                ));
                continue;
            }
        };
        let loc = count_non_empty_lines(&bytes);
        files.insert(rel, loc);
    }

    let mut max_loc = 0usize;
    let mut worst_path: Option<String> = None;

    for (path, loc) in &files {
        if *loc > max_loc {
            max_loc = *loc;
            worst_path = Some(path.clone());
        }

        if *loc > cfg.max_loc {
            violations.push(Violation::observation(
                "loc.max_exceeded",
                format!("file exceeds max_loc={} (loc={})", cfg.max_loc, loc),
                Some(path.clone()),
                None,
            ));
        }
    }

    Ok(LocCheckResult {
        violations,
        files_scanned: files.len(),
        files_universe,
        max_loc,
        worst_path,
        loc_per_file: files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ViolationTier;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn count_non_empty_lines_trims_ws_and_crlf() {
        assert_eq!(count_non_empty_lines(b""), 0);
        assert_eq!(count_non_empty_lines(b"\n\n"), 0);
        assert_eq!(count_non_empty_lines(b"  \n\t\r\nx\r\ny\n"), 2);
    }

    #[test]
    fn loc_marks_over_limit_as_observation() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join("crates/x")).unwrap();

        let file_path = repo_root.join("crates/x/lib.rs");
        let mut f = fs::File::create(&file_path).unwrap();
        writeln!(f, "fn a() {{}}\nfn b() {{}}\n").unwrap();

        let cfg = LocCheckConfigV2 {
            id: "loc".to_string(),
            max_loc: 1,
            include_globs: vec!["crates/**/*.rs".to_string()],
            exclude_globs: vec![],
            baseline_path: ".agents/mcp/compas/baselines/loc.json".to_string(),
        };

        let r = run_loc_check(repo_root, &cfg).unwrap();
        assert!(r.violations.iter().any(|v| v.code == "loc.max_exceeded"));
        assert!(
            r.violations
                .iter()
                .all(|v| v.tier == ViolationTier::Observation)
        );
    }

    #[test]
    fn loc_reports_per_file_map_for_quality_delta() {
        let dir = tempdir().unwrap();
        let repo_root = dir.path();
        fs::create_dir_all(repo_root.join("crates/x")).unwrap();

        let file_path = repo_root.join("crates/x/lib.rs");
        let mut f = fs::File::create(&file_path).unwrap();
        writeln!(f, "fn a() {{}}\nfn b() {{}}\n").unwrap();

        let cfg = LocCheckConfigV2 {
            id: "loc".to_string(),
            max_loc: 100,
            include_globs: vec!["crates/**/*.rs".to_string()],
            exclude_globs: vec![],
            baseline_path: ".agents/mcp/compas/baselines/loc.json".to_string(),
        };

        let r = run_loc_check(repo_root, &cfg).unwrap();
        assert_eq!(r.files_scanned, 1);
        assert_eq!(r.files_universe, 1);
        assert!(r.loc_per_file.contains_key("crates/x/lib.rs"));
    }
}
