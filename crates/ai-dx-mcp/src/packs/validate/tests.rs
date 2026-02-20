use super::validate_packs;
use super::{PACKS_DIR_REL, PACKS_LOCK_REL};
use std::fs;
use tempfile::tempdir;

#[test]
fn packs_dir_requires_lock_file() {
    let dir = tempdir().unwrap();
    let repo_root = dir.path();
    fs::create_dir_all(repo_root.join(PACKS_DIR_REL)).unwrap();

    let v = validate_packs(repo_root);
    assert!(v.iter().any(|x| x.code == "packs.lock_missing"));
}

#[test]
fn lock_requires_sha256_for_non_builtin_sources() {
    let dir = tempdir().unwrap();
    let repo_root = dir.path();
    fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();

    fs::write(
        repo_root.join(PACKS_LOCK_REL),
        r#"
version = 1
packs = [
  { id = "org/custom", source = "file:/tmp/pack" },
]
"#,
    )
    .unwrap();

    let v = validate_packs(repo_root);
    assert!(v.iter().any(|x| x.code == "packs.lock_sha256_required"));
}

#[test]
fn pack_manifest_requires_explicit_disabled_for_unwired_ids() {
    let dir = tempdir().unwrap();
    let repo_root = dir.path();
    fs::create_dir_all(repo_root.join(PACKS_DIR_REL).join("rust")).unwrap();
    fs::create_dir_all(repo_root.join(".agents/mcp/compas")).unwrap();

    // satisfy lock requirement
    fs::write(repo_root.join(PACKS_LOCK_REL), "version = 1\npacks = []\n").unwrap();

    fs::write(
        repo_root.join(PACKS_DIR_REL).join("rust/pack.toml"),
        r#"
[pack]
id = "rust"
version = "0.1.0"
description = "Rust defaults for compas validate tests"
languages = ["rust"]

[[tools]]
[tools.tool]
id = "rust-test"
description = "cargo test"
command = "cargo"
args = ["test"]

[canonical_tools]
test = ["rust-test"]
disabled = ["build", "lint", "fmt"]
"#,
    )
    .unwrap();

    let v = validate_packs(repo_root);
    assert!(v.iter().any(|x| x.code == "packs.canonical_tools_invalid"));
}
