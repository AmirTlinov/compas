use crate::api::Violation;
use crate::packs::schema::PacksLockV1;
use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn mk_violation(code: &str, message: String, path: Option<String>) -> Violation {
    Violation::blocking(code, message, path, None)
}

pub(super) fn validate_packs_lock(
    repo_root: &Path,
    packs_dir_present: bool,
    lock_rel: &str,
    pack_id_re: Regex,
    is_valid_sha256_hex: fn(&str) -> bool,
) -> Vec<Violation> {
    let lock_path = repo_root.join(lock_rel);

    let mut violations: Vec<Violation> = vec![];
    if !lock_path.is_file() {
        if packs_dir_present {
            violations.push(mk_violation(
                "packs.lock_missing",
                "packs.lock is required when packs directory exists".to_string(),
                Some(lock_rel.to_string()),
            ));
        }
        return violations;
    }

    let raw = match fs::read_to_string(&lock_path) {
        Ok(v) => v,
        Err(e) => {
            violations.push(mk_violation(
                "packs.lock_read_failed",
                format!("failed to read packs.lock: {e}"),
                Some(lock_rel.to_string()),
            ));
            return violations;
        }
    };

    let parsed: PacksLockV1 = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            violations.push(mk_violation(
                "packs.lock_parse_failed",
                format!("failed to parse packs.lock: {e}"),
                Some(lock_rel.to_string()),
            ));
            return violations;
        }
    };

    if parsed.version != 1 {
        violations.push(mk_violation(
            "packs.lock_version_unsupported",
            format!("unsupported packs.lock version={}", parsed.version),
            Some(lock_rel.to_string()),
        ));
    }

    let mut seen_ids: BTreeSet<String> = BTreeSet::new();

    for entry in &parsed.packs {
        if entry.id.trim().is_empty() || !pack_id_re.is_match(entry.id.trim()) {
            violations.push(mk_violation(
                "packs.lock_invalid_id",
                format!("invalid pack id in packs.lock: {:?}", entry.id),
                Some(lock_rel.to_string()),
            ));
        }
        if !seen_ids.insert(entry.id.clone()) {
            violations.push(mk_violation(
                "packs.lock_duplicate_id",
                format!("duplicate pack id in packs.lock: {:?}", entry.id),
                Some(lock_rel.to_string()),
            ));
        }

        let source = entry.source.trim();
        if source.is_empty() {
            violations.push(mk_violation(
                "packs.lock_invalid_source",
                format!("pack {:?} has empty source", entry.id),
                Some(lock_rel.to_string()),
            ));
        }

        // Determinism/offline contract: anything non-builtin must be pinned.
        if !source.starts_with("builtin:") {
            match entry
                .sha256
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                None => violations.push(mk_violation(
                    "packs.lock_sha256_required",
                    format!(
                        "pack {:?} requires sha256 pin (source={source:?})",
                        entry.id
                    ),
                    Some(lock_rel.to_string()),
                )),
                Some(sha) if !is_valid_sha256_hex(sha) => violations.push(mk_violation(
                    "packs.lock_sha256_invalid",
                    format!("pack {:?} has invalid sha256: {:?}", entry.id, sha),
                    Some(lock_rel.to_string()),
                )),
                Some(_) => {}
            }
        }

        // Offline guarantee for network sources: require resolved_path.
        if source.starts_with("http://") || source.starts_with("https://") {
            match entry
                .resolved_path
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                None => violations.push(mk_violation(
                    "packs.lock_resolved_path_required",
                    format!(
                        "pack {:?} requires resolved_path for network source",
                        entry.id
                    ),
                    Some(lock_rel.to_string()),
                )),
                Some(p) => {
                    let as_path = PathBuf::from(p);
                    if as_path.is_absolute() || p.split('/').any(|seg| seg == "..") {
                        violations.push(mk_violation(
                            "packs.lock_resolved_path_invalid",
                            format!("pack {:?} has unsafe resolved_path: {:?}", entry.id, p),
                            Some(lock_rel.to_string()),
                        ));
                    }
                }
            }
        }
    }

    violations
}
