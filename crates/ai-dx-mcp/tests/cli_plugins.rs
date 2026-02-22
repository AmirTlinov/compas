use serde_json::Value;

fn write_registry_fixture(root: &std::path::Path) {
    let scripts_dir = root.join("scripts");
    std::fs::create_dir_all(&scripts_dir).expect("mkdir scripts");
    let script = r#"#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import sys

PLUGIN_FILES = {
    "spec-adr-gate": [
        ".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml",
        "tools/custom/spec-adr-gate/tool.toml",
    ],
    "sast-semgrep-gate": [
        ".agents/mcp/compas/plugins/sast-semgrep-gate/plugin.toml",
        "tools/custom/sast-semgrep-gate/tool.toml",
    ],
}
PACKS = {"security": ["sast-semgrep-gate"]}


def parse_csv(raw: str) -> list[str]:
    if not raw:
        return []
    return [x.strip() for x in raw.split(",") if x.strip()]


def dedupe(values: list[str]) -> list[str]:
    out = []
    seen = set()
    for v in values:
        if v not in seen:
            seen.add(v)
            out.append(v)
    return out


def cmd_install(args: argparse.Namespace) -> int:
    plugin_ids = parse_csv(args.plugins)
    for pack in parse_csv(args.packs):
        plugin_ids.extend(PACKS.get(pack, []))
    plugin_ids = dedupe(plugin_ids)
    if not plugin_ids:
        print("no plugins requested", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "argv": sys.argv[1:],
                "cwd": os.getcwd(),
                "plugins": plugin_ids,
                "dry_run": bool(args.dry_run),
            }
        )
    )
    return 0


def cmd_list(args: argparse.Namespace) -> int:
    rows = [{"id": pid, "status": "community"} for pid in sorted(PLUGIN_FILES.keys())]
    if args.json:
        print(json.dumps(rows))
    else:
        for row in rows:
            print(row["id"])
    return 0


def cmd_packs(args: argparse.Namespace) -> int:
    rows = [{"id": pid, "plugins": plugins} for pid, plugins in sorted(PACKS.items())]
    if args.json:
        print(json.dumps(rows))
    else:
        for row in rows:
            print(row["id"])
    return 0


def cmd_info(args: argparse.Namespace) -> int:
    plugin_id = args.plugin_id
    if plugin_id not in PLUGIN_FILES:
        print(f"unknown plugin: {plugin_id}", file=sys.stderr)
        return 2
    print(json.dumps({"id": plugin_id, "install_files": PLUGIN_FILES[plugin_id]}))
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)

    sp_install = sub.add_parser("install")
    sp_install.add_argument("--plugins", default="")
    sp_install.add_argument("--packs", default="")
    sp_install.add_argument("--target", default=".")
    sp_install.add_argument("--dry-run", action="store_true")
    sp_install.add_argument("--allow-missing-tools", action="store_true")
    sp_install.set_defaults(func=cmd_install)

    sp_list = sub.add_parser("list")
    sp_list.add_argument("--json", action="store_true")
    sp_list.set_defaults(func=cmd_list)

    sp_packs = sub.add_parser("packs")
    sp_packs.add_argument("--json", action="store_true")
    sp_packs.set_defaults(func=cmd_packs)

    sp_info = sub.add_parser("info")
    sp_info.add_argument("plugin_id")
    sp_info.set_defaults(func=cmd_info)

    return p


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
"#;
    std::fs::write(scripts_dir.join("compas_plugins.py"), script).expect("write script");
}

fn write_registry_state(repo_root: &std::path::Path, payload: &serde_json::Value) {
    let state_path = repo_root.join(".agents/mcp/compas/plugins/.registry_state.json");
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir state parent");
    }
    std::fs::write(
        state_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(payload).expect("serialize state")
        ),
    )
    .expect("write state");
}

fn write_file(repo_root: &std::path::Path, rel: &str) {
    let path = repo_root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir file parent");
    }
    std::fs::write(path, "fixture\n").expect("write fixture file");
}

fn extract_installer_argv(stdout: &[u8]) -> Vec<String> {
    let parsed: Value = serde_json::from_slice(stdout).expect("parse installer json");
    parsed
        .get("argv")
        .and_then(|v| v.as_array())
        .expect("installer argv array")
        .iter()
        .map(|v| v.as_str().expect("argv string").to_string())
        .collect()
}

fn assert_manifest_only_flags_removed(argv: &[String], pubkey: &str) {
    assert!(
        !argv.iter().any(|v| v == "--allow-unsigned"),
        "legacy argv leaked --allow-unsigned: {argv:?}"
    );
    assert!(
        !argv.iter().any(|v| v == "--allow-experimental"),
        "legacy argv leaked --allow-experimental: {argv:?}"
    );
    assert!(
        !argv.iter().any(|v| v == "--allow-deprecated"),
        "legacy argv leaked --allow-deprecated: {argv:?}"
    );
    assert!(
        !argv.iter().any(|v| v == "--pubkey"),
        "legacy argv leaked --pubkey: {argv:?}"
    );
    assert!(
        !argv.iter().any(|v| v == pubkey),
        "legacy argv leaked pubkey value: {argv:?}"
    );
}

#[test]
fn cli_plugins_install_uses_cached_local_registry() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");

    write_registry_fixture(registry_root.path());

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "install", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args(["--plugins", "spec-adr-gate", "--dry-run"])
        .output()
        .expect("run plugins install");

    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let parsed: Value = serde_json::from_slice(&out.stdout).expect("parse json");
    assert_eq!(
        parsed
            .get("argv")
            .and_then(|v| v.get(0))
            .and_then(|v| v.as_str()),
        Some("install")
    );
    let argv = parsed
        .get("argv")
        .and_then(|v| v.as_array())
        .expect("argv array");
    assert!(
        argv.iter().any(|v| v.as_str() == Some("--plugins")),
        "argv={argv:?}"
    );
    assert!(
        argv.iter().any(|v| v.as_str() == Some("spec-adr-gate")),
        "argv={argv:?}"
    );
    assert_eq!(
        parsed.get("cwd").and_then(|v| v.as_str()),
        Some(repo_root.path().to_string_lossy().as_ref())
    );

    let cache_registry = cache_root.path().join("compas/plugins/registry");
    assert!(cache_registry.is_dir(), "cache dir missing");
    let mut installer_found = false;
    for entry in walkdir::WalkDir::new(&cache_registry) {
        let entry = entry.expect("walk cache");
        if entry.file_type().is_file() && entry.file_name() == "compas_plugins.py" {
            installer_found = true;
            break;
        }
    }
    assert!(installer_found, "cached installer not found");
}

#[test]
fn cli_plugins_legacy_installer_strips_manifest_only_flags_from_install() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());

    let pubkey = "/tmp/compas-test-pubkey.pem";
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "install", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args([
            "--plugins",
            "spec-adr-gate",
            "--allow-unsigned",
            "--allow-experimental",
            "--allow-deprecated",
            "--pubkey",
            pubkey,
            "--dry-run",
        ])
        .output()
        .expect("run plugins install");

    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let argv = extract_installer_argv(&out.stdout);
    assert_manifest_only_flags_removed(&argv, pubkey);
    assert!(
        argv.iter().any(|v| v == "--plugins"),
        "argv missing --plugins: {argv:?}"
    );
    assert!(
        argv.iter().any(|v| v == "spec-adr-gate"),
        "argv missing requested plugin id: {argv:?}"
    );
}

#[test]
fn cli_plugins_legacy_installer_strips_manifest_only_flags_from_update() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());
    write_registry_state(
        repo_root.path(),
        &serde_json::json!({
            "registry": "compas-plugin-registry",
            "source_root": "/tmp/fixture",
            "plugins": ["spec-adr-gate"],
            "packs": [],
            "files": [".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml"],
        }),
    );

    let pubkey = "/tmp/compas-test-pubkey.pem";
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "update", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args([
            "--allow-unsigned",
            "--allow-experimental",
            "--allow-deprecated",
            "--pubkey",
            pubkey,
            "--dry-run",
        ])
        .output()
        .expect("run plugins update");

    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let argv = extract_installer_argv(&out.stdout);
    assert_manifest_only_flags_removed(&argv, pubkey);
    assert_eq!(argv.first().map(String::as_str), Some("install"));
    assert!(
        argv.iter().any(|v| v == "--plugins"),
        "argv missing --plugins: {argv:?}"
    );
    assert!(
        argv.iter().any(|v| v == "spec-adr-gate"),
        "argv missing inferred plugin id from state: {argv:?}"
    );
}

#[test]
fn cli_plugins_legacy_installer_ignores_manifest_only_flags_for_list_and_info() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());

    let pubkey = "/tmp/compas-test-pubkey.pem";
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");

    let list_out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "list", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args([
            "--allow-unsigned",
            "--allow-experimental",
            "--allow-deprecated",
            "--pubkey",
            pubkey,
            "--json",
        ])
        .output()
        .expect("run plugins list");

    assert!(
        list_out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&list_out.stdout),
        String::from_utf8_lossy(&list_out.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&list_out.stdout).expect("parse list json");
    assert!(!rows.is_empty());

    let info_out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "info", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args([
            "spec-adr-gate",
            "--allow-unsigned",
            "--allow-experimental",
            "--allow-deprecated",
            "--pubkey",
            pubkey,
        ])
        .output()
        .expect("run plugins info");

    assert!(
        info_out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&info_out.stdout),
        String::from_utf8_lossy(&info_out.stderr)
    );
    let payload: Value = serde_json::from_slice(&info_out.stdout).expect("parse info json");
    assert_eq!(
        payload.get("id").and_then(|v| v.as_str()),
        Some("spec-adr-gate")
    );
}

#[test]
fn cli_plugins_update_uses_state_when_targets_omitted() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());
    write_registry_state(
        repo_root.path(),
        &serde_json::json!({
            "registry": "compas-plugin-registry",
            "source_root": "/tmp/fixture",
            "plugins": ["spec-adr-gate"],
            "packs": [],
            "files": [".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml"],
        }),
    );

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "update", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .arg("--dry-run")
        .output()
        .expect("run plugins update");

    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let argv = parsed
        .get("argv")
        .and_then(|v| v.as_array())
        .expect("argv array");
    assert_eq!(argv.first().and_then(|v| v.as_str()), Some("install"));
    assert!(
        argv.iter().any(|v| v.as_str() == Some("--plugins")),
        "argv={argv:?}"
    );
    assert!(
        argv.iter().any(|v| v.as_str() == Some("spec-adr-gate")),
        "argv={argv:?}"
    );
}

#[test]
fn cli_plugins_uninstall_removes_files_and_updates_state() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());

    let spec_plugin = ".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml";
    let spec_tool = "tools/custom/spec-adr-gate/tool.toml";
    let sast_plugin = ".agents/mcp/compas/plugins/sast-semgrep-gate/plugin.toml";
    let sast_tool = "tools/custom/sast-semgrep-gate/tool.toml";
    write_file(repo_root.path(), spec_plugin);
    write_file(repo_root.path(), spec_tool);
    write_file(repo_root.path(), sast_plugin);
    write_file(repo_root.path(), sast_tool);

    write_registry_state(
        repo_root.path(),
        &serde_json::json!({
            "registry": "compas-plugin-registry",
            "source_root": "/tmp/fixture",
            "plugins": ["spec-adr-gate", "sast-semgrep-gate"],
            "packs": [],
            "files": [spec_plugin, spec_tool, sast_plugin, sast_tool],
        }),
    );

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "uninstall", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .args(["--plugins", "spec-adr-gate"])
        .output()
        .expect("run plugins uninstall");

    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("parse uninstall json");
    assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert!(
        !repo_root.path().join(spec_plugin).exists(),
        "spec plugin file must be removed"
    );
    assert!(
        !repo_root.path().join(spec_tool).exists(),
        "spec tool file must be removed"
    );
    assert!(
        repo_root.path().join(sast_plugin).exists(),
        "sast plugin file should stay"
    );
    assert!(
        repo_root.path().join(sast_tool).exists(),
        "sast tool file should stay"
    );

    let state_path = repo_root
        .path()
        .join(".agents/mcp/compas/plugins/.registry_state.json");
    let state_raw = std::fs::read_to_string(state_path).expect("read updated state");
    let state: Value = serde_json::from_str(&state_raw).expect("parse updated state");
    let plugins = state
        .get("plugins")
        .and_then(|v| v.as_array())
        .expect("plugins array");
    assert_eq!(plugins.len(), 1, "plugins={plugins:?}");
    assert_eq!(plugins[0].as_str(), Some("sast-semgrep-gate"));
}

#[test]
fn cli_plugins_doctor_reports_missing_state_files() {
    let repo_root = tempfile::tempdir().expect("temp repo");
    let registry_root = tempfile::tempdir().expect("temp registry");
    let cache_root = tempfile::tempdir().expect("temp cache");
    write_registry_fixture(registry_root.path());
    write_registry_state(
        repo_root.path(),
        &serde_json::json!({
            "registry": "compas-plugin-registry",
            "source_root": "/tmp/fixture",
            "plugins": ["spec-adr-gate"],
            "packs": [],
            "files": [".agents/mcp/compas/plugins/spec-adr-gate/plugin.toml"],
        }),
    );

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache_root.path())
        .args(["plugins", "doctor", "--registry"])
        .arg(registry_root.path())
        .args(["--repo-root"])
        .arg(repo_root.path())
        .output()
        .expect("run plugins doctor");

    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("parse doctor json");
    assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(false));
    let missing = parsed
        .get("missing_files")
        .and_then(|v| v.as_array())
        .expect("missing_files");
    assert_eq!(missing.len(), 1, "missing_files={missing:?}");
}

#[test]
fn cli_plugins_rejects_unknown_subcommand() {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .args(["plugins", "typo"])
        .output()
        .expect("run plugins typo");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown plugins command"),
        "stderr missing expected message: {stderr}"
    );
}
