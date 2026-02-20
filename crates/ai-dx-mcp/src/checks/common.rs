use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

fn build_globset(globs: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in globs {
        let g = Glob::new(p).map_err(|e| format!("invalid glob {:?}: {e}", p))?;
        b.add(g);
    }
    b.build()
        .map_err(|e| format!("failed to build globset: {e}"))
}

fn should_descend(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
    )
}

pub(crate) fn normalize_rel(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn collect_candidate_files(
    repo_root: &Path,
    include_globs: &[String],
    exclude_globs: &[String],
) -> Result<Vec<(String, PathBuf)>, String> {
    let include = if include_globs.is_empty() {
        None
    } else {
        Some(build_globset(include_globs)?)
    };
    let exclude = if exclude_globs.is_empty() {
        None
    } else {
        Some(build_globset(exclude_globs)?)
    };

    let mut out: Vec<(String, PathBuf)> = vec![];
    for entry in WalkDir::new(repo_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = normalize_rel(repo_root, entry.path()) else {
            continue;
        };
        if let Some(inc) = &include
            && !inc.is_match(&rel)
        {
            continue;
        }
        if let Some(exc) = &exclude
            && exc.is_match(&rel)
        {
            continue;
        }
        out.push((rel, entry.path().to_path_buf()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub(crate) fn is_probably_code_file(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|s| s.to_str()),
        Some("rs")
            | Some("py")
            | Some("js")
            | Some("jsx")
            | Some("ts")
            | Some("tsx")
            | Some("go")
            | Some("c")
            | Some("h")
            | Some("cc")
            | Some("cpp")
            | Some("cxx")
            | Some("hpp")
            | Some("cs")
    )
}
