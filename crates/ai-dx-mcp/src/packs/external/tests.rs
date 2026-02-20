use super::{PACKS_LOCK_REL, VENDOR_DIR_REL, upsert_packs_lock, vendor_pack_archive_bytes};
use crate::hash::sha256_hex;
use crate::packs::schema::{PackLockEntryV1, PacksLockV1};
use flate2::{Compression, write::GzEncoder};
use std::fs;
use std::io::Write;
use std::path::Path;
use tar::{Builder, EntryType, Header};
use tempfile::tempdir;

fn build_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());

    for (path, bytes) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, *path, *bytes)
            .expect("append tar entry");
    }

    builder.finish().expect("finish tar");
    builder.into_inner().expect("tar bytes")
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).expect("gzip write");
    enc.finish().expect("gzip finish")
}

#[test]
fn vendor_pack_archive_bytes_vends_pack_and_returns_lock_entry() {
    let repo = tempdir().expect("temp repo");

    let pack_id = "org/custom";
    let pack_toml = format!(
        r#"
[pack]
id = "{pack_id}"
version = "1.2.3"
description = "Custom pack used for external vendoring tests"
languages = ["rust"]
"#
    );

    let tar = build_tar(&[
        ("pack/pack.toml", pack_toml.as_bytes()),
        ("pack/README.md", b"hello"),
    ]);
    let gz = gzip(&tar);
    let sha = sha256_hex(&gz);

    let source = "https://example.com/pack.tar.gz";
    let (manifest, entry) =
        vendor_pack_archive_bytes(repo.path(), source, &sha, &gz).expect("vendor ok");

    assert_eq!(manifest.pack.id, pack_id);
    assert_eq!(entry.id, pack_id);
    assert_eq!(entry.source, source);
    assert_eq!(entry.sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(entry.version.as_deref(), Some("1.2.3"));

    let expected_resolved = format!("{VENDOR_DIR_REL}/{pack_id}");
    assert_eq!(
        entry.resolved_path.as_deref(),
        Some(expected_resolved.as_str())
    );

    let vendor_dir = repo.path().join(VENDOR_DIR_REL).join("org").join("custom");
    assert!(vendor_dir.join("pack.toml").is_file());
    assert!(vendor_dir.join("README.md").is_file());
}

#[test]
fn upsert_packs_lock_creates_updates_and_sorts() {
    let repo = tempdir().expect("temp repo");

    let e_b = PackLockEntryV1 {
        id: "b".to_string(),
        source: "builtin:b".to_string(),
        sha256: None,
        resolved_path: None,
        version: Some("0.1.0".to_string()),
    };
    let e_a_v1 = PackLockEntryV1 {
        id: "a".to_string(),
        source: "builtin:a".to_string(),
        sha256: None,
        resolved_path: None,
        version: Some("0.1.0".to_string()),
    };
    let e_a_v2 = PackLockEntryV1 {
        id: "a".to_string(),
        source: "builtin:a".to_string(),
        sha256: None,
        resolved_path: None,
        version: Some("0.2.0".to_string()),
    };

    upsert_packs_lock(repo.path(), e_b).expect("insert b");
    upsert_packs_lock(repo.path(), e_a_v1).expect("insert a");
    upsert_packs_lock(repo.path(), e_a_v2).expect("update a");

    let lock_path = repo.path().join(PACKS_LOCK_REL);
    let raw = fs::read_to_string(&lock_path).expect("read packs.lock");
    let lock: PacksLockV1 = toml::from_str(&raw).expect("parse packs.lock");

    assert_eq!(lock.version, 1);
    assert_eq!(lock.packs.len(), 2);
    assert_eq!(lock.packs[0].id, "a");
    assert_eq!(lock.packs[0].version.as_deref(), Some("0.2.0"));
    assert_eq!(lock.packs[1].id, "b");
}

#[test]
fn vendor_pack_archive_bytes_fails_on_sha_mismatch() {
    let repo = tempdir().expect("temp repo");

    let tar = build_tar(&[(
        "pack/pack.toml",
        br#"
[pack]
id = "rust"
version = "0.0.1"
description = "Rust pack for sha mismatch test"
"#,
    )]);
    let gz = gzip(&tar);
    let mut sha = sha256_hex(&gz);
    sha.replace_range(0..1, if &sha[0..1] == "0" { "1" } else { "0" });

    let err = vendor_pack_archive_bytes(repo.path(), "src", &sha, &gz).unwrap_err();
    assert!(err.contains("sha256 mismatch"), "{err}");
}

#[test]
fn safe_unpack_entry_path_rejects_parent_dir_and_absolute_paths() {
    assert!(super::safe_unpack_entry_path(Path::new("../pack.toml")).is_err());
    assert!(super::safe_unpack_entry_path(Path::new("/abs/pack.toml")).is_err());
}

#[test]
fn extract_pack_archive_to_dir_rejects_symlink_entries() {
    let repo = tempdir().expect("temp repo");
    let dest = repo.path().join("out");

    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_mode(0o777);
    header.set_size(0);
    header.set_link_name("target").expect("set link name");
    header.set_cksum();
    builder
        .append_data(&mut header, "pack/link", &[][..])
        .expect("append symlink entry");
    builder.finish().expect("finish tar");
    let tar = builder.into_inner().expect("tar bytes");

    let err = super::extract_pack_archive_to_dir(&tar, &dest).unwrap_err();
    assert!(err.contains("unsupported tar entry type"), "{err}");
}

#[test]
fn vendor_pack_archive_bytes_fails_on_multiple_pack_toml() {
    let repo = tempdir().expect("temp repo");

    let tar = build_tar(&[
        (
            "a/pack.toml",
            br#"
[pack]
id = "a"
version = "0.0.1"
description = "Pack A used to trigger multiple-pack.toml error"
"#,
        ),
        (
            "b/pack.toml",
            br#"
[pack]
id = "b"
version = "0.0.1"
description = "Pack B used to trigger multiple-pack.toml error"
"#,
        ),
    ]);
    let gz = gzip(&tar);
    let sha = sha256_hex(&gz);

    let err = vendor_pack_archive_bytes(repo.path(), "src", &sha, &gz).unwrap_err();
    assert!(err.contains("multiple pack.toml"), "{err}");
}
