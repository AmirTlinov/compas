use crate::api::Violation;
use crate::config::SupplyChainCheckConfigV2;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug)]
pub struct SupplyChainCheckResult {
    pub violations: Vec<Violation>,
}

#[derive(Debug, Default)]
struct ManifestScan {
    rust_manifest_paths: Vec<String>,
    rust_lock_present: bool,
    node_manifest_paths: Vec<String>,
    node_lock_present: bool,
    python_manifest_paths: Vec<String>,
    python_lock_present: bool,
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

fn normalize_rel_path(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn extract_first_quoted_value(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let rest = &s[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn looks_prerelease_version(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    lower.contains("-alpha") || lower.contains("-beta") || lower.contains("-rc")
}

fn scan_cargo_prerelease_deps(raw: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut in_dependencies = false;

    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            let section = t[1..t.len() - 1].to_ascii_lowercase();
            in_dependencies = section == "dependencies"
                || section == "dev-dependencies"
                || section == "build-dependencies"
                || section.ends_with(".dependencies")
                || section.ends_with(".dev-dependencies")
                || section.ends_with(".build-dependencies");
            continue;
        }
        if !in_dependencies {
            continue;
        }

        let Some((name, rhs)) = t.split_once('=') else {
            continue;
        };
        let dep_name = name.trim().trim_matches('"').to_string();
        if dep_name.is_empty() {
            continue;
        }

        let rhs = rhs.trim();
        let version = if rhs.starts_with('"') {
            extract_first_quoted_value(rhs)
        } else if rhs.starts_with('{') {
            rhs.find("version")
                .and_then(|idx| extract_first_quoted_value(&rhs[idx..]))
        } else {
            None
        };

        let Some(version) = version else {
            continue;
        };
        if !looks_prerelease_version(&version) {
            continue;
        }
        if seen.insert(dep_name.clone()) {
            out.push((dep_name, version));
        }
    }

    out
}

fn scan_package_json_prerelease_deps(raw: &str) -> Result<Vec<(String, String)>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("failed to parse package.json: {e}"))?;

    let mut out: Vec<(String, String)> = vec![];
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        let Some(map) = parsed.get(section).and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, version_val) in map {
            let Some(version) = version_val.as_str() else {
                continue;
            };
            if !looks_prerelease_version(version) {
                continue;
            }
            if seen.insert(name.clone()) {
                out.push((name.clone(), version.to_string()));
            }
        }
    }

    Ok(out)
}

fn scan_manifests(repo_root: &Path) -> ManifestScan {
    let mut scan = ManifestScan::default();
    for entry in WalkDir::new(repo_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(rel) = normalize_rel_path(repo_root, path) else {
            continue;
        };
        match entry.file_name().to_string_lossy().as_ref() {
            "Cargo.toml" => scan.rust_manifest_paths.push(rel),
            "Cargo.lock" => scan.rust_lock_present = true,
            "package.json" => scan.node_manifest_paths.push(rel),
            "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lockb" | "bun.lock" => {
                scan.node_lock_present = true;
            }
            "pyproject.toml" | "Pipfile" | "setup.py" => scan.python_manifest_paths.push(rel),
            "poetry.lock" | "uv.lock" | "Pipfile.lock" | "requirements.txt" => {
                scan.python_lock_present = true;
            }
            _ => {}
        }
    }
    scan.rust_manifest_paths.sort();
    scan.node_manifest_paths.sort();
    scan.python_manifest_paths.sort();
    scan
}

pub fn run_supply_chain_check(
    repo_root: &Path,
    _cfg: &SupplyChainCheckConfigV2,
) -> SupplyChainCheckResult {
    let scan = scan_manifests(repo_root);
    let mut violations: Vec<Violation> = vec![];

    if !scan.rust_manifest_paths.is_empty() && !scan.rust_lock_present {
        violations.push(Violation::blocking(
            "supply_chain.lockfile_missing",
            "rust manifests detected but Cargo.lock is missing",
            Some("Cargo.lock".to_string()),
            Some(json!({
                "ecosystem": "rust",
                "manifests": scan.rust_manifest_paths,
            })),
        ));
    }

    if !scan.node_manifest_paths.is_empty() && !scan.node_lock_present {
        violations.push(Violation::blocking(
            "supply_chain.lockfile_missing",
            "node manifests detected but lockfile is missing",
            Some("package.json".to_string()),
            Some(json!({
                "ecosystem": "node",
                "manifests": scan.node_manifest_paths,
            })),
        ));
    }

    if !scan.python_manifest_paths.is_empty() && !scan.python_lock_present {
        violations.push(Violation::blocking(
            "supply_chain.lockfile_missing",
            "python manifests detected but lockfile is missing",
            Some("pyproject.toml".to_string()),
            Some(json!({
                "ecosystem": "python",
                "manifests": scan.python_manifest_paths,
            })),
        ));
    }

    for rel in &scan.rust_manifest_paths {
        let full = repo_root.join(rel);
        let raw = match std::fs::read_to_string(&full) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "supply_chain.read_failed",
                    format!("failed to read manifest {rel}: {e}"),
                    Some(rel.clone()),
                    Some(json!({ "ecosystem": "rust" })),
                ));
                continue;
            }
        };
        for (dep, version) in scan_cargo_prerelease_deps(&raw) {
            violations.push(Violation::blocking(
                "supply_chain.prerelease_dependency",
                format!("prerelease rust dependency is forbidden: {dep}={version}"),
                Some(rel.clone()),
                Some(json!({
                    "ecosystem": "rust",
                    "dependency": dep,
                    "version": version,
                })),
            ));
        }
    }

    for rel in &scan.node_manifest_paths {
        let full = repo_root.join(rel);
        let raw = match std::fs::read_to_string(&full) {
            Ok(v) => v,
            Err(e) => {
                violations.push(Violation::blocking(
                    "supply_chain.read_failed",
                    format!("failed to read manifest {rel}: {e}"),
                    Some(rel.clone()),
                    Some(json!({ "ecosystem": "node" })),
                ));
                continue;
            }
        };
        match scan_package_json_prerelease_deps(&raw) {
            Ok(deps) => {
                for (dep, version) in deps {
                    violations.push(Violation::blocking(
                        "supply_chain.prerelease_dependency",
                        format!("prerelease node dependency is forbidden: {dep}={version}"),
                        Some(rel.clone()),
                        Some(json!({
                            "ecosystem": "node",
                            "dependency": dep,
                            "version": version,
                        })),
                    ));
                }
            }
            Err(e) => violations.push(Violation::blocking(
                "supply_chain.manifest_parse_failed",
                e,
                Some(rel.clone()),
                Some(json!({ "ecosystem": "node" })),
            )),
        }
    }

    SupplyChainCheckResult { violations }
}
