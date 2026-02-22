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
    let cache = tempfile::tempdir().expect("temp cache");
    std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache.path())
        .args(args)
        .output()
        .expect("run compas")
}

fn run_compas_env(args: &[String], envs: &[(&str, &str)]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let cache = tempfile::tempdir().expect("temp cache");
    let mut cmd = std::process::Command::new(bin);
    cmd.env("XDG_CACHE_HOME", cache.path());
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("run compas with env")
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

#[test]
fn manifest_update_infers_lockfile_targets_and_completes() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    let install_args = vec![
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
    let install = run_compas(&install_args);
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let update_args = vec![
        "plugins".to_string(),
        "update".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--allow-unsigned".to_string(),
    ];
    let update = run_compas(&update_args);
    assert!(
        update.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );
    let payload: Value = serde_json::from_slice(&update.stdout).expect("parse update payload");
    let plugins = payload
        .get("plugins")
        .and_then(|v| v.as_array())
        .expect("plugins array");
    assert!(
        plugins.iter().any(|v| v.as_str() == Some("spec-adr-gate")),
        "plugins={plugins:?}"
    );
}

#[test]
fn manifest_uninstall_blocks_on_type_drift_without_force() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    let install_args = vec![
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
    let install = run_compas(&install_args);
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let drift_file = repo_root.join(".agents/mcp/compas/plugins/spec-adr-gate/README.md");
    std::fs::remove_file(&drift_file).expect("remove original file");
    std::fs::create_dir_all(&drift_file).expect("replace managed file with directory");
    write_file(&drift_file.join("nested.txt"), "drift payload\n");

    let uninstall_args = vec![
        "plugins".to_string(),
        "uninstall".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
    ];
    let blocked = run_compas(&uninstall_args);
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&blocked.stdout),
        String::from_utf8_lossy(&blocked.stderr)
    );
    let payload: Value = serde_json::from_slice(&blocked.stdout).expect("parse uninstall payload");
    let modified = payload
        .get("modified_files")
        .and_then(|v| v.as_array())
        .expect("modified_files");
    assert!(
        modified
            .iter()
            .any(|v| v.as_str() == Some(".agents/mcp/compas/plugins/spec-adr-gate/README.md")),
        "modified_files={modified:?}"
    );
    assert!(
        drift_file.is_dir(),
        "type-drift directory must remain after blocked uninstall"
    );
}

#[test]
fn manifest_uninstall_rolls_back_when_lockfile_commit_fails() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    let install_args = vec![
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
    let install = run_compas(&install_args);
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let uninstall_args = vec![
        "plugins".to_string(),
        "uninstall".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--plugins".to_string(),
        "spec-adr-gate".to_string(),
        "--allow-unsigned".to_string(),
    ];

    let injected = run_compas_env(
        &uninstall_args,
        &[("COMPAS_TEST_FAIL_UNINSTALL_LOCK_COMMIT", "1")],
    );
    assert_eq!(
        injected.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&injected.stdout),
        String::from_utf8_lossy(&injected.stderr)
    );
    let stderr = String::from_utf8_lossy(&injected.stderr);
    assert!(
        stderr.contains("rollback executed"),
        "stderr should contain rollback marker, got: {stderr}"
    );

    // File should be restored because uninstall transaction rolled back.
    assert!(
        repo_root
            .join(".agents/mcp/compas/plugins/spec-adr-gate/README.md")
            .is_file(),
        "managed plugin file must be restored after rollback"
    );
    assert!(
        repo_root
            .join(".agents/mcp/compas/plugins.lock.json")
            .is_file(),
        "lockfile must remain after rollback"
    );

    let clean = run_compas(&uninstall_args);
    assert!(
        clean.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(
        !repo_root
            .join(".agents/mcp/compas/plugins/spec-adr-gate")
            .exists(),
        "plugin directory should be removed by clean uninstall"
    );
    assert!(
        !repo_root
            .join(".agents/mcp/compas/plugins.lock.json")
            .exists(),
        "lockfile should be removed when nothing managed remains"
    );
}

#[test]
fn manifest_doctor_detects_type_drift_and_unknown_symlink() {
    let workspace = tempfile::tempdir().expect("workspace");
    let repo_root = workspace.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("mkdir repo");
    let manifest_path = build_manifest_registry_fixture(workspace.path());

    let install_args = vec![
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
    let install = run_compas(&install_args);
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let managed_file = repo_root.join(".agents/mcp/compas/plugins/spec-adr-gate/README.md");
    std::fs::remove_file(&managed_file).expect("remove managed file");
    std::fs::create_dir_all(&managed_file).expect("replace managed file with dir");
    write_file(&managed_file.join("nested.txt"), "drift\n");

    let unknown_symlink = repo_root.join(".agents/mcp/compas/plugins/spec-adr-gate/unknown.link");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/tmp", &unknown_symlink).expect("create unknown symlink");
    #[cfg(not(unix))]
    write_file(&unknown_symlink, "fallback\n");

    let doctor_args = vec![
        "plugins".to_string(),
        "doctor".to_string(),
        "--registry".to_string(),
        manifest_path.to_string_lossy().to_string(),
        "--repo-root".to_string(),
        repo_root.to_string_lossy().to_string(),
        "--allow-unsigned".to_string(),
    ];
    let doctor = run_compas(&doctor_args);
    assert_eq!(
        doctor.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("parse doctor payload");
    let modified = payload
        .get("modified_files")
        .and_then(|v| v.as_array())
        .expect("modified_files");
    assert!(
        modified
            .iter()
            .any(|v| v.as_str() == Some(".agents/mcp/compas/plugins/spec-adr-gate/README.md")),
        "modified_files={modified:?}"
    );

    let unknown = payload
        .get("unknown_files")
        .and_then(|v| v.as_array())
        .expect("unknown_files");
    assert!(
        unknown.iter().any(|v| {
            v.as_str() == Some(".agents/mcp/compas/plugins/spec-adr-gate/unknown.link")
        }),
        "unknown_files={unknown:?}"
    );
}
