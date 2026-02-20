use super::canonical_tools::{validate_canonical_tools_config, validate_pack_gates};
use crate::api::Violation;
use crate::packs::schema::PackManifestV1;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn mk_violation(code: &str, message: String, path: Option<String>) -> Violation {
    Violation::blocking(code, message, path, None)
}

fn tool_ids_from_manifest(manifest: &PackManifestV1) -> BTreeSet<String> {
    manifest.tools.iter().map(|t| t.tool.id.clone()).collect()
}

fn tools_by_id_from_manifest(manifest: &PackManifestV1) -> BTreeMap<String, String> {
    manifest
        .tools
        .iter()
        .map(|t| (t.tool.id.clone(), t.tool.description.clone()))
        .collect()
}

pub(super) fn validate_pack_manifests(
    repo_root: &Path,
    packs_dir_rel: &str,
    normalize_rel_path: fn(&Path, &Path) -> Option<String>,
    pack_id_re: Regex,
) -> Vec<Violation> {
    let packs_dir = repo_root.join(packs_dir_rel);
    if !packs_dir.is_dir() {
        return vec![];
    }

    let tool_id_re = crate::repo_strict::id_regex();
    let mut seen_pack_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut violations: Vec<Violation> = vec![];

    for entry in WalkDir::new(&packs_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "pack.toml" {
            continue;
        }

        let path = entry.path();
        let rel = normalize_rel_path(repo_root, path).unwrap_or_else(|| path.display().to_string());
        let raw = match fs::read_to_string(path) {
            Ok(v) => v,
            Err(e) => {
                violations.push(mk_violation(
                    "packs.pack_read_failed",
                    format!("failed to read pack manifest: {e}"),
                    Some(rel),
                ));
                continue;
            }
        };

        let manifest: PackManifestV1 = match toml::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                violations.push(mk_violation(
                    "packs.pack_parse_failed",
                    format!("failed to parse pack manifest: {e}"),
                    Some(rel),
                ));
                continue;
            }
        };

        let pack_id = manifest.pack.id.trim().to_string();
        if pack_id.is_empty() || !pack_id_re.is_match(&pack_id) {
            violations.push(mk_violation(
                "packs.pack_invalid_id",
                format!("invalid pack.id: {:?}", manifest.pack.id),
                Some(rel.clone()),
            ));
        }
        if let Some(prev) = seen_pack_ids.insert(pack_id.clone(), rel.clone()) {
            violations.push(mk_violation(
                "packs.pack_duplicate_id",
                format!("duplicate pack.id={pack_id:?} (prev={prev})"),
                Some(rel.clone()),
            ));
        }

        let desc_len = manifest.pack.description.trim().chars().count();
        if !(12..=220).contains(&desc_len) {
            violations.push(mk_violation(
                "packs.pack_description_invalid",
                "pack.description length must be 12..220 chars".to_string(),
                Some(rel.clone()),
            ));
        }

        let tool_ids = tool_ids_from_manifest(&manifest);
        let tools_by_id = tools_by_id_from_manifest(&manifest);
        for (tool_id, description) in &tools_by_id {
            if !tool_id_re.is_match(tool_id) {
                violations.push(mk_violation(
                    "packs.pack_tool_invalid_id",
                    format!("invalid tool.id in pack: {tool_id:?}"),
                    Some(rel.clone()),
                ));
            }
            let desc = description.trim();
            if desc.is_empty() {
                violations.push(mk_violation(
                    "packs.pack_tool_description_required",
                    format!("tool {tool_id:?} description is required"),
                    Some(rel.clone()),
                ));
            } else {
                let len = desc.chars().count();
                if !(12..=220).contains(&len) {
                    violations.push(mk_violation(
                        "packs.pack_tool_description_invalid",
                        format!("tool {tool_id:?} description length must be 12..220 chars"),
                        Some(rel.clone()),
                    ));
                }
            }
        }

        if let Some(canon) = &manifest.canonical_tools {
            let problems = validate_canonical_tools_config(canon, &tool_ids);
            if !problems.is_empty() {
                violations.push(mk_violation(
                    "packs.canonical_tools_invalid",
                    format!("canonical_tools invalid: {}", problems.join("; ")),
                    Some(rel.clone()),
                ));
            }

            if let Some(gates) = &manifest.gates {
                let gate_problems = validate_pack_gates(gates, canon);
                if !gate_problems.is_empty() {
                    violations.push(mk_violation(
                        "packs.gates_invalid",
                        format!("gates invalid: {}", gate_problems.join("; ")),
                        Some(rel.clone()),
                    ));
                }
            }
        } else if manifest.gates.is_some() {
            violations.push(mk_violation(
                "packs.canonical_tools_required",
                "gates present but canonical_tools is missing".to_string(),
                Some(rel.clone()),
            ));
        }
    }

    violations
}
