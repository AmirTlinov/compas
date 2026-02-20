use crate::api::{ApiError, InitPlan};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const ALLOWED_PREFIXES: [&str; 2] = [".agents/mcp/compas/", "tools/custom/"];

fn api_err(code: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        code: code.to_string(),
        message: message.into(),
    }
}

fn normalize_rel_path(p: &str) -> Result<String, ApiError> {
    let trimmed = p.trim();
    if trimmed.is_empty() {
        return Err(api_err(
            "init.plan_path_empty",
            "plan path must be non-empty",
        ));
    }
    if trimmed.contains('\\') {
        return Err(api_err(
            "init.plan_path_invalid",
            format!("backslashes are not allowed in plan paths: {trimmed:?}"),
        ));
    }

    let as_path = PathBuf::from(trimmed);
    if as_path.is_absolute() {
        return Err(api_err(
            "init.plan_path_invalid",
            format!("absolute paths are forbidden in plan: {trimmed:?}"),
        ));
    }
    for c in as_path.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(_) => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(api_err(
                    "init.plan_path_invalid",
                    format!("unsafe path in plan (.. or root): {trimmed:?}"),
                ));
            }
        }
    }

    Ok(trimmed.to_string())
}

fn ensure_allowed_scope(rel: &str) -> Result<(), ApiError> {
    if ALLOWED_PREFIXES.iter().any(|p| rel.starts_with(p)) {
        Ok(())
    } else {
        Err(api_err(
            "init.plan_path_forbidden",
            format!(
                "path is outside init allowlist; allowed_prefixes={:?}; got={:?}",
                ALLOWED_PREFIXES, rel
            ),
        ))
    }
}

fn ensure_no_symlink_components(repo_root: &Path, rel: &str) -> Result<(), ApiError> {
    let mut cur = repo_root.to_path_buf();
    for c in Path::new(rel).components() {
        cur.push(c.as_os_str());
        match fs::symlink_metadata(&cur) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(api_err(
                        "init.plan_path_symlink",
                        format!("unsafe symlink path component in init plan: {:?}", cur),
                    ));
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    continue;
                }
                return Err(api_err(
                    "init.symlink_check_failed",
                    format!("failed to stat path component {:?}: {e}", cur),
                ));
            }
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), ApiError> {
    let parent = path.parent().ok_or_else(|| {
        api_err(
            "init.write_failed",
            format!("path has no parent dir: {:?}", path),
        )
    })?;
    fs::create_dir_all(parent).map_err(|e| {
        api_err(
            "init.write_failed",
            format!("failed to create dir {:?}: {e}", parent),
        )
    })?;

    if path.is_dir() {
        return Err(api_err(
            "init.write_failed",
            format!("refusing to overwrite directory with file: {:?}", path),
        ));
    }

    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }

    fs::write(&tmp, content.as_bytes()).map_err(|e| {
        api_err(
            "init.write_failed",
            format!("failed to write tmp file {:?}: {e}", tmp),
        )
    })?;

    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Windows rename() won't overwrite; do best-effort fallback (still fail-closed).
            if path.exists() {
                fs::remove_file(path).map_err(|e2| {
                    api_err(
                        "init.write_failed",
                        format!("failed to remove existing file {:?}: {e2}", path),
                    )
                })?;
                fs::rename(&tmp, path).map_err(|e3| {
                    api_err(
                        "init.write_failed",
                        format!("failed to rename tmp file into place: {e3}"),
                    )
                })?;
                Ok(())
            } else {
                Err(api_err(
                    "init.write_failed",
                    format!("failed to rename tmp file into place: {e}"),
                ))
            }
        }
    }
}

fn delete_path(path: &Path) -> Result<(), ApiError> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| {
            api_err(
                "init.delete_failed",
                format!("failed to remove dir {:?}: {e}", path),
            )
        })?;
    } else {
        fs::remove_file(path).map_err(|e| {
            api_err(
                "init.delete_failed",
                format!("failed to remove file {:?}: {e}", path),
            )
        })?;
    }
    Ok(())
}

pub(crate) fn apply_plan(repo_root: &Path, plan: &InitPlan) -> Result<(), ApiError> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut skips: BTreeSet<String> = BTreeSet::new();
    let mut deletes: BTreeSet<String> = BTreeSet::new();

    for w in &plan.writes {
        let rel = normalize_rel_path(&w.path)?;
        ensure_allowed_scope(&rel)?;
        ensure_no_symlink_components(repo_root, &rel)?;
        if !seen.insert(rel.clone()) {
            return Err(api_err(
                "init.plan_duplicate_path",
                format!("duplicate write path in plan: {rel:?}"),
            ));
        }

        let dest = repo_root.join(&rel);
        if dest.exists() {
            if dest.is_dir() {
                return Err(api_err(
                    "init.write_failed",
                    format!("refusing to overwrite directory with file: {:?}", dest),
                ));
            }
            let existing = fs::read(&dest).map_err(|e| {
                api_err(
                    "init.write_failed",
                    format!("failed to read existing file {:?}: {e}", dest),
                )
            })?;
            if existing == w.content_utf8.as_bytes() {
                skips.insert(rel.clone());
            } else {
                return Err(api_err(
                    "init.write_conflict",
                    format!(
                        "refusing to overwrite existing file with different content: {:?}; delete it (or run init on a clean repo) and retry",
                        dest
                    ),
                ));
            }
        }
    }

    for d in &plan.deletes {
        let rel = normalize_rel_path(d)?;
        ensure_allowed_scope(&rel)?;
        ensure_no_symlink_components(repo_root, &rel)?;
        deletes.insert(rel);
    }

    // Deletes first (so overwrites can re-create).
    for d in &plan.deletes {
        let rel = normalize_rel_path(d)?;
        delete_path(&repo_root.join(rel))?;
    }

    for w in &plan.writes {
        let rel = normalize_rel_path(&w.path)?;
        if skips.contains(&rel) && !deletes.contains(&rel) {
            continue;
        }
        atomic_write(&repo_root.join(rel), &w.content_utf8)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{InitPlan, InitWriteFile};
    use tempfile::tempdir;

    #[test]
    fn apply_plan_writes_files_under_allowlist() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        let plan = InitPlan {
            writes: vec![InitWriteFile {
                path: "tools/custom/x/tool.toml".to_string(),
                content_utf8: "hello".to_string(),
            }],
            deletes: vec![],
        };

        apply_plan(repo, &plan).expect("apply ok");
        assert_eq!(
            fs::read_to_string(repo.join("tools/custom/x/tool.toml")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn apply_plan_rejects_paths_outside_allowlist() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        let plan = InitPlan {
            writes: vec![InitWriteFile {
                path: "README.md".to_string(),
                content_utf8: "nope".to_string(),
            }],
            deletes: vec![],
        };

        let err = apply_plan(repo, &plan).unwrap_err();
        assert_eq!(err.code, "init.plan_path_forbidden");
    }

    #[cfg(unix)]
    #[test]
    fn apply_plan_rejects_symlink_path_component() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let repo = dir.path();
        let outside = tempdir().unwrap();

        // `.agents` is a symlink -> would allow writing outside repo_root without a guard.
        symlink(outside.path(), repo.join(".agents")).expect("create symlink");

        let plan = InitPlan {
            writes: vec![InitWriteFile {
                path: ".agents/mcp/compas/plugins/default/plugin.toml".to_string(),
                content_utf8: "x".to_string(),
            }],
            deletes: vec![],
        };

        let err = apply_plan(repo, &plan).unwrap_err();
        assert_eq!(err.code, "init.plan_path_symlink", "{err:?}");
    }
}
