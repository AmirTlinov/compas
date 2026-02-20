#![allow(dead_code)] // Wired by init (TASK-010); keep implementation in-place until used.

use crate::hash::sha256_hex;
use crate::packs::schema::{PackLockEntryV1, PackManifestV1, PacksLockV1};
use flate2::read::GzDecoder;
use regex::Regex;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use tar::Archive;
use walkdir::WalkDir;

const PACKS_LOCK_REL: &str = ".agents/mcp/compas/packs.lock";
const VENDOR_DIR_REL: &str = ".agents/mcp/compas/packs/vendor";
const STAGING_DIR_REL: &str = ".agents/mcp/compas/packs/vendor/_staging";

fn is_valid_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn pack_id_regex() -> Regex {
    // Allow namespaced pack ids like "org/custom".
    Regex::new(r"^[a-z0-9][a-z0-9_-]{1,63}(?:/[a-z0-9][a-z0-9_-]{1,63})*$")
        .expect("valid pack id regex")
}

fn safe_unpack_entry_path(path: &Path) -> Result<(), String> {
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(format!("unsafe tar entry path: {:?}", path));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

fn extract_pack_archive_to_dir(bytes: &[u8], dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("failed to create staging dir: {e}"))?;

    let cursor = Cursor::new(bytes);
    let reader: Box<dyn Read> = if is_gzip(bytes) {
        Box::new(GzDecoder::new(cursor))
    } else {
        Box::new(cursor)
    };

    let mut archive = Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|e| format!("failed to read tar entries: {e}"))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| format!("failed to read tar entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("failed to read tar entry path: {e}"))?
            .to_path_buf();
        safe_unpack_entry_path(&path)?;

        let entry_type = entry.header().entry_type();
        let is_ok = entry_type.is_dir() || entry_type.is_file();
        if !is_ok {
            return Err(format!("unsupported tar entry type: {:?}", entry_type));
        }

        entry
            .unpack_in(dest)
            .map_err(|e| format!("failed to unpack tar entry {:?}: {e}", path))?;
    }

    Ok(())
}

fn find_single_pack_toml(root: &Path) -> Result<PathBuf, String> {
    let mut found: Vec<PathBuf> = vec![];
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() && entry.file_name() == "pack.toml" {
            found.push(entry.path().to_path_buf());
        }
    }
    if found.is_empty() {
        return Err("pack.toml not found in extracted archive".to_string());
    }
    if found.len() > 1 {
        return Err(format!(
            "multiple pack.toml files found in extracted archive: {}",
            found.len()
        ));
    }
    Ok(found.remove(0))
}

fn pack_id_to_vendor_dir(repo_root: &Path, pack_id: &str) -> Result<(PathBuf, String), String> {
    let id_re = pack_id_regex();
    if !id_re.is_match(pack_id) {
        return Err(format!("invalid pack.id: {pack_id:?}"));
    }

    let mut rel = String::from(VENDOR_DIR_REL);
    rel.push('/');
    rel.push_str(pack_id);

    let mut dest = repo_root.join(VENDOR_DIR_REL);
    for seg in pack_id.split('/') {
        dest.push(seg);
    }
    Ok((dest, rel))
}

pub(crate) fn vendor_pack_archive_bytes(
    repo_root: &Path,
    source: &str,
    expected_sha256: &str,
    archive_bytes: &[u8],
) -> Result<(PackManifestV1, PackLockEntryV1), String> {
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if !is_valid_sha256_hex(&expected) {
        return Err(format!("invalid expected sha256 hex: {expected_sha256:?}"));
    }

    let actual = sha256_hex(archive_bytes);
    if actual != expected {
        return Err(format!(
            "sha256 mismatch for pack source={source:?}: expected={expected}, actual={actual}"
        ));
    }

    let staging_root = repo_root.join(STAGING_DIR_REL).join(&actual);
    if staging_root.exists() {
        fs::remove_dir_all(&staging_root)
            .map_err(|e| format!("failed to clear staging dir {:?}: {e}", staging_root))?;
    }

    extract_pack_archive_to_dir(archive_bytes, &staging_root)?;

    let pack_toml = find_single_pack_toml(&staging_root)?;
    let pack_root = pack_toml
        .parent()
        .ok_or_else(|| "pack.toml has no parent dir".to_string())?;
    let raw = fs::read_to_string(&pack_toml)
        .map_err(|e| format!("failed to read extracted pack.toml {:?}: {e}", pack_toml))?;
    let manifest: PackManifestV1 =
        toml::from_str(&raw).map_err(|e| format!("failed to parse pack.toml: {e}"))?;

    let (vendor_dir, resolved_rel) = pack_id_to_vendor_dir(repo_root, &manifest.pack.id)?;
    if vendor_dir.exists() {
        fs::remove_dir_all(&vendor_dir)
            .map_err(|e| format!("failed to remove existing vendor dir {:?}: {e}", vendor_dir))?;
    }
    if let Some(parent) = vendor_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create vendor parent dir {:?}: {e}", parent))?;
    }

    fs::rename(pack_root, &vendor_dir).map_err(|e| {
        format!(
            "failed to move extracted pack to vendor dir (from={:?}, to={:?}): {e}",
            pack_root, vendor_dir
        )
    })?;

    // Clean staging root (best-effort).
    let _ = fs::remove_dir_all(&staging_root);

    let entry = PackLockEntryV1 {
        id: manifest.pack.id.clone(),
        source: source.to_string(),
        sha256: Some(actual),
        resolved_path: Some(resolved_rel),
        version: Some(manifest.pack.version.clone()),
    };
    Ok((manifest, entry))
}

pub(crate) fn upsert_packs_lock(repo_root: &Path, entry: PackLockEntryV1) -> Result<(), String> {
    let lock_path = repo_root.join(PACKS_LOCK_REL);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create lock dir: {e}"))?;
    }

    let mut lock: PacksLockV1 = if lock_path.is_file() {
        let raw = fs::read_to_string(&lock_path)
            .map_err(|e| format!("failed to read packs.lock {:?}: {e}", lock_path))?;
        toml::from_str(&raw).map_err(|e| format!("failed to parse packs.lock: {e}"))?
    } else {
        PacksLockV1 {
            version: 1,
            packs: vec![],
        }
    };

    if lock.version != 1 {
        return Err(format!("unsupported packs.lock version={}", lock.version));
    }

    lock.packs.retain(|p| p.id != entry.id);
    lock.packs.push(entry);
    lock.packs.sort_by(|a, b| a.id.cmp(&b.id));

    let out = toml::to_string_pretty(&lock)
        .map_err(|e| format!("failed to serialize packs.lock: {e}"))?;
    fs::write(&lock_path, out).map_err(|e| format!("failed to write packs.lock: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
