use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct RegistryFixture {
    manifest_path: std::path::PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_file(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

fn write_registry_fixture(root: &std::path::Path) -> RegistryFixture {
    let payload_root = root.join("payload/registry");
    let spec_dir = payload_root.join("plugins/spec-adr-gate");
    let exp_dir = payload_root.join("plugins/experimental-plugin");

    write_file(
        &spec_dir.join("plugin.toml"),
        "[plugin]\nid = \"spec-adr-gate\"\ndescription = \"Spec ADR gate\"\n",
    );
    write_file(
        &exp_dir.join("plugin.toml"),
        "[plugin]\nid = \"experimental-plugin\"\ndescription = \"Experimental plugin\"\n",
    );

    let archive_name = "registry.v1.tar.gz";
    let archive_path = root.join(archive_name);

    let tar_gz = std::fs::File::create(&archive_path).expect("create archive");
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all("registry", &payload_root)
        .expect("append payload");
    let enc = tar.into_inner().expect("finish tar builder");
    enc.finish().expect("finish gzip");

    let archive_bytes = std::fs::read(&archive_path).expect("read archive");
    let archive_sha = sha256_hex(&archive_bytes);

    let manifest = serde_json::json!({
        "schema": "compas.registry.manifest.v1",
        "registry_version": "test-1",
        "archive": {
            "name": archive_name,
            "sha256": archive_sha,
        },
        "plugins": [
            {
                "id": "spec-adr-gate",
                "aliases": ["spec"],
                "path": "plugins/spec-adr-gate",
                "status": "community",
                "owner": "test",
                "description": "Spec gate plugin",
                "tier": "community",
                "capabilities": ["adr", "gate"],
                "requires": [],
                "runtime_kind": "tool-backed",
                "cost_class": "medium",
                "artifacts_produced": [],
                "package": {
                    "version": "1.0.0",
                    "type": "tool-backed",
                    "maturity": "stable",
                    "runtime": "python3",
                    "portable": true,
                    "languages": ["python"],
                    "entrypoint": "scripts/spec.py",
                    "license": "MIT"
                }
            },
            {
                "id": "experimental-plugin",
                "aliases": [],
                "path": "plugins/experimental-plugin",
                "status": "community",
                "owner": "test",
                "description": "Experimental plugin",
                "tier": "experimental",
                "capabilities": ["example", "lint"],
                "requires": [],
                "runtime_kind": "tool-backed",
                "cost_class": "medium",
                "artifacts_produced": [],
                "package": {
                    "version": "0.1.0",
                    "type": "tool-backed",
                    "maturity": "experimental",
                    "runtime": "python3",
                    "portable": true,
                    "languages": ["python"],
                    "entrypoint": "scripts/exp.py",
                    "license": "MIT"
                }
            }
        ],
        "packs": [
            {
                "id": "ai-core",
                "description": "Core AI quality plugin set",
                "plugins": ["spec-adr-gate"],
                "capabilities": ["adr", "gate"],
                "requires": [],
                "runtime_kind": "tool-backed",
                "cost_class": "medium"
            }
        ]
    });

    let manifest_path = root.join("registry.manifest.v1.json");
    std::fs::write(
        &manifest_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).expect("serialize manifest")
        ),
    )
    .expect("write manifest");

    RegistryFixture { manifest_path }
}

fn run_plugins_cmd(
    repo_root: &std::path::Path,
    fixture: &RegistryFixture,
    args: &[&str],
) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let cache_home = repo_root.join(".tmp-xdg-cache");
    std::fs::create_dir_all(&cache_home).expect("mkdir cache");
    let mut cmd = std::process::Command::new(bin);
    cmd.env("XDG_CACHE_HOME", &cache_home);
    cmd.args(["plugins"])
        .args(args)
        .args(["--registry"])
        .arg(&fixture.manifest_path)
        .args(["--repo-root"])
        .arg(repo_root)
        .args(["--allow-unsigned"]);
    cmd.output().expect("run plugins command")
}

#[test]
fn plugins_install_requires_admin_lane() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let fixture = write_registry_fixture(registry_root.path());

    let out = run_plugins_cmd(
        repo_root.path(),
        &fixture,
        &["install", "--plugins", "spec-adr-gate", "--dry-run"],
    );

    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("requires explicit --admin-lane"),
        "stderr missing admin-lane message: {stderr}"
    );
}

#[test]
fn plugins_manifest_discovery_commands_work() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let fixture = write_registry_fixture(registry_root.path());

    let list = run_plugins_cmd(repo_root.path(), &fixture, &["list", "--json"]);
    assert!(
        list.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&list.stdout).expect("parse list json");
    assert!(
        rows.iter()
            .any(|r| r.get("id") == Some(&Value::String("spec-adr-gate".into())))
    );

    let packs = run_plugins_cmd(repo_root.path(), &fixture, &["packs", "--json"]);
    assert!(
        packs.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&packs.stdout),
        String::from_utf8_lossy(&packs.stderr)
    );
    let pack_rows: Vec<Value> = serde_json::from_slice(&packs.stdout).expect("parse packs json");
    assert!(
        pack_rows
            .iter()
            .any(|r| r.get("id") == Some(&Value::String("ai-core".into())))
    );

    let info = run_plugins_cmd(repo_root.path(), &fixture, &["info", "spec"]);
    assert!(
        info.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&info.stdout),
        String::from_utf8_lossy(&info.stderr)
    );
    let payload: Value = serde_json::from_slice(&info.stdout).expect("parse info json");
    assert_eq!(
        payload.get("id"),
        Some(&Value::String("spec-adr-gate".into()))
    );
}

#[test]
fn plugins_install_update_uninstall_admin_lane_flow() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let fixture = write_registry_fixture(registry_root.path());

    let install = run_plugins_cmd(
        repo_root.path(),
        &fixture,
        &["install", "--admin-lane", "--plugins", "spec-adr-gate"],
    );
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );
    let install_payload: Value = serde_json::from_slice(&install.stdout).expect("install json");
    assert_eq!(install_payload.get("ok"), Some(&Value::Bool(true)));

    let installed_plugin = repo_root
        .path()
        .join(".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml");
    assert!(installed_plugin.is_file(), "installed plugin file missing");

    let lockfile = repo_root
        .path()
        .join(".agents/mcp/compas/plugins.lock.json");
    assert!(lockfile.is_file(), "lockfile missing after install");

    let update = run_plugins_cmd(
        repo_root.path(),
        &fixture,
        &["update", "--admin-lane", "--dry-run"],
    );
    assert!(
        update.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );
    let update_payload: Value = serde_json::from_slice(&update.stdout).expect("update json");
    let plugins = update_payload
        .get("plugins")
        .and_then(|v| v.as_array())
        .expect("update plugins");
    assert!(
        plugins.iter().any(|v| v.as_str() == Some("spec-adr-gate")),
        "update must infer lockfile plugins: {plugins:?}"
    );

    let uninstall = run_plugins_cmd(
        repo_root.path(),
        &fixture,
        &["uninstall", "--admin-lane", "--plugins", "spec-adr-gate"],
    );
    assert!(
        uninstall.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&uninstall.stdout),
        String::from_utf8_lossy(&uninstall.stderr)
    );
    let uninstall_payload: Value =
        serde_json::from_slice(&uninstall.stdout).expect("uninstall json");
    assert_eq!(uninstall_payload.get("ok"), Some(&Value::Bool(true)));
    assert!(
        !installed_plugin.exists(),
        "plugin file should be removed by uninstall"
    );
    assert!(
        !lockfile.exists(),
        "lockfile should be removed when all plugins are uninstalled"
    );
}

#[test]
fn plugins_doctor_reports_missing_managed_files() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let fixture = write_registry_fixture(registry_root.path());

    let install = run_plugins_cmd(
        repo_root.path(),
        &fixture,
        &["install", "--admin-lane", "--plugins", "spec-adr-gate"],
    );
    assert!(
        install.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let managed_file = repo_root
        .path()
        .join(".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml");
    std::fs::remove_file(&managed_file).expect("remove managed file");

    let doctor = run_plugins_cmd(repo_root.path(), &fixture, &["doctor"]);
    assert_eq!(
        doctor.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("doctor json");
    assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
    let missing = payload
        .get("missing_files")
        .and_then(|v| v.as_array())
        .expect("missing_files array");
    assert!(
        missing
            .iter()
            .any(|v| v.as_str() == Some(".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml")),
        "doctor must report missing managed file: {missing:?}"
    );
}
