#![allow(dead_code)] // Wired by init (TASK-010); keep implementation in-place until used.

use crate::packs::builtin::load_builtin_pack_manifests;
use crate::packs::schema::{PackDetectorV1, PackManifestV1};
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodePackageManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
}

pub(crate) fn detect_node_package_manager(repo_root: &Path) -> Option<NodePackageManager> {
    // Lockfile-driven and deterministic. Priority is explicit.
    if repo_root.join("pnpm-lock.yaml").is_file() {
        return Some(NodePackageManager::Pnpm);
    }
    if repo_root.join("yarn.lock").is_file() {
        return Some(NodePackageManager::Yarn);
    }
    if repo_root.join("bun.lockb").is_file() || repo_root.join("bun.lock").is_file() {
        return Some(NodePackageManager::Bun);
    }
    if repo_root.join("package-lock.json").is_file()
        || repo_root.join("npm-shrinkwrap.json").is_file()
    {
        return Some(NodePackageManager::Npm);
    }
    None
}

fn path_exists(repo_root: &Path, rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }

    // Detector paths support exact paths and glob patterns (e.g. "**/*.csproj").
    // Keep exact-path fast path boring and deterministic.
    if !glob::Pattern::escape(rel).eq(rel) {
        let pattern = repo_root.join(rel);
        let pattern = pattern.to_string_lossy().to_string();
        return glob::glob(&pattern)
            .ok()
            .into_iter()
            .flat_map(|paths| paths.filter_map(Result::ok))
            .filter(|p| !is_generated_noise_path(repo_root, p))
            .any(|p| p.exists());
    }

    repo_root.join(rel).exists()
}

fn is_generated_noise_path(repo_root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(repo_root).unwrap_or(path);
    rel.components().any(|component| match component {
        Component::Normal(seg) => matches!(
            seg.to_str(),
            Some(
                "target"
                    | "node_modules"
                    | "vendor"
                    | ".git"
                    | ".venv"
                    | "venv"
                    | "dist"
                    | "build"
                    | "obj"
                    | "bin"
            )
        ),
        _ => false,
    })
}

pub(crate) fn detector_matches_repo(repo_root: &Path, d: &PackDetectorV1) -> bool {
    if d.any_paths.is_empty() && d.all_paths.is_empty() && d.none_paths.is_empty() {
        return false;
    }

    if d.none_paths.iter().any(|p| path_exists(repo_root, p)) {
        return false;
    }
    if d.all_paths.iter().any(|p| !path_exists(repo_root, p)) {
        return false;
    }
    if !d.any_paths.is_empty() && !d.any_paths.iter().any(|p| path_exists(repo_root, p)) {
        return false;
    }

    true
}

pub(crate) fn pack_matches_repo(repo_root: &Path, pack: &PackManifestV1) -> bool {
    if pack.detectors.is_empty() {
        return false;
    }
    pack.detectors
        .iter()
        .any(|d| detector_matches_repo(repo_root, d))
}

pub(crate) fn load_builtin_packs() -> Result<BTreeMap<String, PackManifestV1>, String> {
    load_builtin_pack_manifests()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detector_semantics_any_all_none() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        std::fs::write(repo.join("a"), "x").unwrap();
        std::fs::create_dir_all(repo.join("dir")).unwrap();

        let d = PackDetectorV1 {
            id: "d".to_string(),
            any_paths: vec!["a".to_string()],
            all_paths: vec!["dir".to_string()],
            none_paths: vec!["missing".to_string()],
        };
        assert!(detector_matches_repo(repo, &d));

        let mut d2 = d.clone();
        d2.none_paths = vec!["a".to_string()];
        assert!(!detector_matches_repo(repo, &d2));
    }

    #[test]
    fn node_pm_detects_by_lockfile_priority() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        assert_eq!(detect_node_package_manager(repo), None);

        std::fs::write(repo.join("package-lock.json"), "{}").unwrap();
        assert_eq!(
            detect_node_package_manager(repo),
            Some(NodePackageManager::Npm)
        );

        std::fs::write(repo.join("yarn.lock"), "").unwrap();
        assert_eq!(
            detect_node_package_manager(repo),
            Some(NodePackageManager::Yarn)
        );

        std::fs::write(repo.join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            detect_node_package_manager(repo),
            Some(NodePackageManager::Pnpm)
        );
    }

    #[test]
    fn detector_supports_glob_patterns() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        std::fs::create_dir_all(repo.join("src/App")).unwrap();
        std::fs::write(repo.join("src/App/App.csproj"), "<Project />").unwrap();

        let d = PackDetectorV1 {
            id: "dotnet-csproj".to_string(),
            any_paths: vec!["**/*.csproj".to_string()],
            all_paths: vec![],
            none_paths: vec!["**/*.sln".to_string()],
        };
        assert!(detector_matches_repo(repo, &d));

        std::fs::write(repo.join("compas.sln"), "").unwrap();
        assert!(!detector_matches_repo(repo, &d));
    }

    #[test]
    fn detector_glob_ignores_generated_noise_paths() {
        let dir = tempdir().unwrap();
        let repo = dir.path();

        std::fs::create_dir_all(repo.join("target/generated")).unwrap();
        std::fs::write(
            repo.join("target/generated/Noise.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\" />",
        )
        .unwrap();

        let d = PackDetectorV1 {
            id: "dotnet-csproj".to_string(),
            any_paths: vec!["**/*.csproj".to_string()],
            all_paths: vec![],
            none_paths: vec![],
        };
        assert!(
            !detector_matches_repo(repo, &d),
            "generated target/** paths must not trigger detector"
        );

        std::fs::create_dir_all(repo.join("src/App")).unwrap();
        std::fs::write(
            repo.join("src/App/App.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\" />",
        )
        .unwrap();
        assert!(
            detector_matches_repo(repo, &d),
            "real source csproj must still trigger detector"
        );
    }
}
