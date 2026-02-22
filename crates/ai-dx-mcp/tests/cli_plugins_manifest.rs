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

fn build_manifest_registry_fixture(root: &Path) -> PathBuf {
    let payload_root = root.join("registry_payload");
    let plugin_dir = payload_root.join("plugins/spec-adr-gate");
    write_file(&plugin_dir.join("README.md"), "spec-adr plugin fixture\n");
    write_file(&plugin_dir.join("plugin.toml"), "id='spec-adr-gate'\n");

    let archive_name = "compas_plugins-fixture.tar.gz";
    let archive_path = root.join(archive_name);
    let tar_gz = std::fs::File::create(&archive_path).expect("create archive");
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all("compas_plugins-fixture", &payload_root)
        .expect("append dir");
    let enc = tar.into_inner().expect("finalize tar");
    let _ = enc.finish().expect("finalize gzip");

    let manifest_path = root.join("registry.manifest.v1.json");
    let manifest = serde_json::json!({
        "schema": "compas.registry.manifest.v1",
        "registry_version": "fixture-1",
        "archive": {
            "name": archive_name,
            "sha256": sha256_file(&archive_path),
        },
        "plugins": [
            {
                "id": "spec-adr-gate",
                "aliases": ["spec-gate"],
                "path": "plugins/spec-adr-gate",
                "status": "community",
                "description": "Fixture plugin for manifest integration tests",
                "package": {
                    "version": "0.1.0",
                    "type": "script",
                    "maturity": "stable",
                    "runtime": "python3",
                    "portable": true,
                    "languages": ["agnostic"],
                    "entrypoint": "README.md",
                    "license": "MIT"
                }
            }
        ],
        "packs": [
            {
                "id": "core",
                "description": "Core fixture pack",
                "plugins": ["spec-adr-gate"]
            }
        ]
    });
    std::fs::write(
        &manifest_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).expect("serialize manifest")
        ),
    )
    .expect("write manifest");
    manifest_path
}

fn run_compas(args: &[String]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    std::process::Command::new(bin)
        .args(args)
        .output()
        .expect("run compas")
}

#[test]
fn manifest_install_blocks_on_drift_without_force_and_recovers_with_force() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    let base_args = vec![
        "plugins".to_string(),
        "install".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
    ];

    let first = run_compas(&base_args);
    assert!(
        first.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    let installed_file = repo_root.join(".agents/mcp/compas/plugins/spec-adr-gate/README.md");
    assert!(installed_file.is_file(), "installed file missing");
    std::fs::write(&installed_file, "tampered\n").expect("tamper file");

    let second = run_compas(&base_args);
    assert_eq!(
        second.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let second_payload: Value = serde_json::from_slice(&second.stdout).expect("parse json");
    assert_eq!(
        second_payload.get("blocked").and_then(|v| v.as_bool()),
        Some(true)
    );
    let modified = second_payload
        .get("preflight")
        .and_then(|v| v.get("modified_files"))
        .and_then(|v| v.as_array())
        .expect("modified_files");
    assert!(
        modified
            .iter()
            .any(|v| v.as_str() == Some(".agents/mcp/compas/plugins/spec-adr-gate/README.md")),
        "modified_files={modified:?}"
    );

    let mut force_args = base_args.clone();
    force_args.push("--force".to_string());
    let third = run_compas(&force_args);
    assert!(
        third.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&third.stdout),
        String::from_utf8_lossy(&third.stderr)
    );
    let restored = std::fs::read_to_string(&installed_file).expect("read restored file");
    assert_eq!(restored, "spec-adr plugin fixture\n");
}

#[test]
fn manifest_install_blocks_on_unmanaged_plugin_dirs_without_force() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    write_file(
        &repo_root.join(".agents/mcp/compas/plugins/manual-custom/README.md"),
        "manual plugin\n",
    );

    let base_args = vec![
        "plugins".to_string(),
        "install".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
    ];

    let blocked = run_compas(&base_args);
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&blocked.stdout),
        String::from_utf8_lossy(&blocked.stderr)
    );
    let blocked_payload: Value = serde_json::from_slice(&blocked.stdout).expect("parse json");
    let unmanaged = blocked_payload
        .get("preflight")
        .and_then(|v| v.get("unmanaged_plugin_dirs"))
        .and_then(|v| v.as_array())
        .expect("unmanaged_plugin_dirs");
    assert!(
        unmanaged
            .iter()
            .any(|v| v.as_str() == Some("manual-custom")),
        "unmanaged={unmanaged:?}"
    );

    let mut force_args = base_args.clone();
    force_args.push("--force".to_string());
    let forced = run_compas(&force_args);
    assert!(
        forced.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&forced.stdout),
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(
        repo_root
            .join(".agents/mcp/compas/plugins/manual-custom/README.md")
            .is_file(),
        "manual plugin dir should remain untouched after forced install"
    );
}
