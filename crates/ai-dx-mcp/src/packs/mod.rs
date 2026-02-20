pub mod schema;

mod builtin;
mod engine;
#[cfg(feature = "external_packs")]
mod external;
mod validate;

#[allow(unused_imports)] // Wired by init (TASK-010); keep exports stable meanwhile.
pub(crate) use engine::{
    NodePackageManager, detect_node_package_manager, load_builtin_packs, pack_matches_repo,
};
pub(crate) use validate::validate_packs;

/// Vendor a pack archive into the repo (`.agents/mcp/compas/packs/vendor/*`) and return the parsed
/// manifest + lock entry.
///
/// compas-lite (`--no-default-features`) fails closed with a clear error.
#[allow(dead_code)] // Wired by future init slice (external packs).
pub(crate) fn vendor_pack_archive_bytes(
    repo_root: &std::path::Path,
    source: &str,
    expected_sha256: &str,
    archive_bytes: &[u8],
) -> Result<(schema::PackManifestV1, schema::PackLockEntryV1), String> {
    #[cfg(feature = "external_packs")]
    {
        external::vendor_pack_archive_bytes(repo_root, source, expected_sha256, archive_bytes)
    }
    #[cfg(not(feature = "external_packs"))]
    {
        let _ = (repo_root, source, expected_sha256, archive_bytes);
        Err("external_packs feature is disabled (compas-lite); rebuild with default-features or --features external_packs".to_string())
    }
}

/// Upsert an entry in `.agents/mcp/compas/packs.lock` (sorted by pack id).
///
/// compas-lite (`--no-default-features`) fails closed with a clear error.
#[allow(dead_code)] // Wired by future init slice (external packs).
pub(crate) fn upsert_packs_lock(
    repo_root: &std::path::Path,
    entry: schema::PackLockEntryV1,
) -> Result<(), String> {
    #[cfg(feature = "external_packs")]
    {
        external::upsert_packs_lock(repo_root, entry)
    }
    #[cfg(not(feature = "external_packs"))]
    {
        let _ = (repo_root, entry);
        Err("external_packs feature is disabled (compas-lite); rebuild with default-features or --features external_packs".to_string())
    }
}

#[cfg(all(test, not(feature = "external_packs")))]
mod lite_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lite_build_fails_closed_on_external_pack_vendoring() {
        let dir = tempdir().unwrap();
        let err = vendor_pack_archive_bytes(dir.path(), "src", "00", b"").unwrap_err();
        assert!(err.contains("external_packs feature is disabled"), "{err}");
    }
}
