use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

fn sha256_file(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read file");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn run_compas(args: &[String]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let cache = tempfile::tempdir().expect("temp cache");
    std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache.path())
        .args(args)
        .output()
        .expect("run compas")
}

fn build_registry_archive(root: &Path) -> PathBuf {
    let payload_root = root.join("payload_root");
    let plugin_dir = payload_root.join("plugins/spec-adr-gate");
    write_file(&plugin_dir.join("README.md"), "spec-adr fixture\n");
    write_file(
        &plugin_dir.join("plugin.toml"),
        "[plugin]\nid='spec-adr-gate'\n",
    );

    let archive_name = "compas_plugins-fixture.tar.gz";
    let archive_path = root.join(archive_name);
    let tar_gz = std::fs::File::create(&archive_path).expect("create archive");
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all("compas_plugins-fixture", &payload_root)
        .expect("append payload");
    let enc = tar.into_inner().expect("finalize tar");
    let _ = enc.finish().expect("finalize gzip");
    archive_path
}

fn write_manifest(
    root: &Path,
    archive_path: &Path,
    tier: &str,
    deprecated_meta_present: bool,
) -> PathBuf {
    let archive_name = archive_path
        .file_name()
        .expect("archive name")
        .to_string_lossy()
        .to_string();

    let mut plugin = serde_json::json!({
        "id": "spec-adr-gate",
        "aliases": ["spec-gate"],
        "path": "plugins/spec-adr-gate",
        "description": "Fixture plugin for tier policy tests",
        "package": {
            "version": "0.1.0",
            "type": "script",
            "maturity": "stable",
            "runtime": "python3",
            "portable": true,
            "languages": ["agnostic"],
            "entrypoint": "README.md",
            "license": "MIT"
        },
        "tier": tier,
        "maintainers": ["AmirTlinov"],
        "tags": ["quality"],
        "compat": { "compas": { "min": "0.1.0", "max": null } }
    });

    if deprecated_meta_present {
        plugin.as_object_mut().expect("plugin object").insert(
            "deprecated".to_string(),
            serde_json::json!({
                "since": "fixture",
                "reason": "fixture deprecated marker",
            }),
        );
    }

    let manifest_path = root.join("registry.manifest.v1.json");
    let manifest = serde_json::json!({
        "schema": "compas.registry.manifest.v1",
        "registry_version": "fixture-1",
        "archive": { "name": archive_name, "sha256": sha256_file(archive_path) },
        "plugins": [ plugin ],
        "packs": [
            { "id": "core", "description": "Fixture pack", "plugins": ["spec-adr-gate"] }
        ]
    });
    write_file(
        &manifest_path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).expect("serialize manifest")
        ),
    );
    manifest_path
}

fn run_install(
    repo_root: &Path,
    manifest_path: &Path,
    extra_flags: &[&str],
) -> std::process::Output {
    let mut args = vec![
        "plugins".to_string(),
        "install".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--".to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
        "--force".to_string(),
    ];
    for flag in extra_flags {
        args.push(flag.to_string());
    }
    run_compas(&args)
}

#[test]
fn install_blocks_experimental_without_allow_flag() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path());
    let manifest_path = write_manifest(workspace.path(), &archive_path, "experimental", false);

    let out = run_install(&repo_root, &manifest_path, &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("json payload");
    assert_eq!(payload.get("blocked").and_then(|v| v.as_bool()), Some(true));
    let reason = payload
        .get("governance")
        .and_then(|v| v.get("blocked_plugins"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        reason.contains("tier=experimental"),
        "unexpected reason: {reason}"
    );
}

#[test]
fn install_allows_experimental_with_allow_flag() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path());
    let manifest_path = write_manifest(workspace.path(), &archive_path, "experimental", false);

    let out = run_install(&repo_root, &manifest_path, &["--allow-experimental"]);
    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn install_blocks_deprecated_without_allow_flag() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path());
    let manifest_path = write_manifest(workspace.path(), &archive_path, "deprecated", true);

    let out = run_install(&repo_root, &manifest_path, &[]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: Value = serde_json::from_slice(&out.stdout).expect("json payload");
    assert_eq!(payload.get("blocked").and_then(|v| v.as_bool()), Some(true));
    let reason = payload
        .get("governance")
        .and_then(|v| v.get("blocked_plugins"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        reason.contains("allow-deprecated"),
        "unexpected reason: {reason}"
    );
}

#[test]
fn install_allows_deprecated_with_allow_flag() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");

    let archive_path = build_registry_archive(workspace.path());
    let manifest_path = write_manifest(workspace.path(), &archive_path, "deprecated", true);

    let out = run_install(&repo_root, &manifest_path, &["--allow-deprecated"]);
    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
