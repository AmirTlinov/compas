use regex::Regex;
use std::path::Path;

use crate::api::Violation;

mod canonical_tools;
mod pack_manifest;
mod packs_lock;

#[cfg(test)]
mod tests;

const PACKS_DIR_REL: &str = ".agents/mcp/compas/packs";
const PACKS_LOCK_REL: &str = ".agents/mcp/compas/packs.lock";

fn normalize_rel_path(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn pack_id_regex() -> Regex {
    // Allow namespaced pack ids like "org/custom".
    Regex::new(r"^[a-z0-9][a-z0-9_-]{1,63}(?:/[a-z0-9][a-z0-9_-]{1,63})*$")
        .expect("valid pack id regex")
}

fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn validate_packs(repo_root: &Path) -> Vec<Violation> {
    let packs_dir_present = repo_root.join(PACKS_DIR_REL).is_dir();
    let lock_present = repo_root.join(PACKS_LOCK_REL).is_file();

    if !(packs_dir_present || lock_present) {
        return vec![];
    }

    let mut out: Vec<Violation> = vec![];
    out.extend(packs_lock::validate_packs_lock(
        repo_root,
        packs_dir_present,
        PACKS_LOCK_REL,
        pack_id_regex(),
        is_valid_sha256_hex,
    ));
    out.extend(pack_manifest::validate_pack_manifests(
        repo_root,
        PACKS_DIR_REL,
        normalize_rel_path,
        pack_id_regex(),
    ));
    out
}
