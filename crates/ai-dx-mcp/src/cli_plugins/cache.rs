use super::super::{PluginsAction, PluginsCli};
use crate::cli::registry_manifest::{ManifestResolved, RegistryManifestV1, RegistryPluginV1};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::SystemTime,
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

const PLUGINS_LOCKFILE_REL_PATH: &str = ".agents/mcp/compas/plugins.lock.json";
const PLUGINS_LOCK_REL_PATH: &str = ".agents/mcp/compas/plugins.lock";
const SUNSET_META_COMPAT_KEY: &str = concat!("deprecat", "ed");
const FLAG_ALLOW_SUNSET: &str = "--allow-sunset";
const FLAG_ALLOW_SUNSET_COMPAT: &str = concat!("--allow-", "deprecat", "ed");
const TIER_EXPERIMENTAL: &str = "experimental";
const TIER_SUNSET: &str = "sunset";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginsLockfileV1 {
    schema: String,
    registry_source: String,
    registry_version: String,
    #[serde(default)]
    manifest_sha256: Option<String>,
    #[serde(default)]
    signature_key_id: Option<String>,
    #[serde(default)]
    plugins: Vec<String>,
    #[serde(default)]
    packs: Vec<String>,
    #[serde(default)]
    files: Vec<PluginsLockfileEntryV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginsLockfileEntryV1 {
    path: String,
    sha256: String,
    #[serde(default)]
    plugin_ids: Vec<String>,
}

fn xdg_cache_home() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        return path;
    }
    if let Some(home) = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    {
        return home.join(".cache");
    }
    std::env::temp_dir().join("compas-cache")
}

fn plugins_cache_root() -> PathBuf {
    xdg_cache_home()
        .join("compas")
        .join("plugins")
        .join("registry")
}

fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

fn ensure_clean_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("failed to clean cache dir {}: {e}", path.display()))?;
    }
    fs::create_dir_all(path)
        .map_err(|e| format!("failed to create cache dir {}: {e}", path.display()))
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("invalid path (missing file name): {}", path.display()))?
        .to_string_lossy()
        .to_string();
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    fs::write(&tmp, bytes).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp, path).map_err(|e| {
        format!(
            "failed to atomically replace {} with {}: {e}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

fn op_nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    sha256_hex(format!("{}|{}", std::process::id(), now).as_bytes())
}

struct PluginsOpLock {
    _file: fs::File,
}

impl Drop for PluginsOpLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

fn acquire_plugins_op_lock(repo_root: &Path) -> Result<PluginsOpLock, String> {
    let lock_path = repo_root.join(PLUGINS_LOCK_REL_PATH);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("failed to open lock file {}: {e}", lock_path.display()))?;
    file.try_lock_exclusive()
        .map_err(|e| format!("another compas plugins operation is running ({e})"))?;
    Ok(PluginsOpLock { _file: file })
}

#[cfg(feature = "full")]
async fn download_url_to_file(url: &str, out_path: &Path) -> Result<(), String> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to download registry from {url}: {e}"))?;
    let response = response
        .error_for_status()
        .map_err(|e| format!("registry download failed for {url}: {e}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read registry body from {url}: {e}"))?;
    fs::write(out_path, &bytes)
        .map_err(|e| format!("failed to write cache archive {}: {e}", out_path.display()))
}

#[cfg(not(feature = "full"))]
async fn download_url_to_file(url: &str, _out_path: &Path) -> Result<(), String> {
    Err(format!(
        "URL registry sources are unavailable in lite build ({url}); use local --registry path"
    ))
}

fn mark_ready(entry: &Path) -> Result<(), String> {
    fs::write(entry.join(".ready"), b"ok\n")
        .map_err(|e| format!("failed to write cache marker in {}: {e}", entry.display()))
}

fn plugins_lockfile_path(repo_root: &Path) -> PathBuf {
    repo_root.join(PLUGINS_LOCKFILE_REL_PATH)
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for value in values {
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    out
}

fn read_plugins_lockfile(repo_root: &Path) -> Result<Option<PluginsLockfileV1>, String> {
    let path = plugins_lockfile_path(repo_root);
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut parsed: PluginsLockfileV1 = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    if parsed.schema != "compas.plugins.lock.v1" {
        return Err(format!(
            "unsupported plugins lockfile schema in {}: {}",
            path.display(),
            parsed.schema
        ));
    }
    parsed.plugins = dedupe_strings(parsed.plugins);
    parsed.packs = dedupe_strings(parsed.packs);
    for entry in &mut parsed.files {
        entry.plugin_ids = dedupe_strings(entry.plugin_ids.clone());
    }
    Ok(Some(parsed))
}

fn write_plugins_lockfile(repo_root: &Path, lock: &PluginsLockfileV1) -> Result<(), String> {
    let path = plugins_lockfile_path(repo_root);
    let json = serde_json::to_string_pretty(lock)
        .map_err(|e| format!("failed to serialize plugins lockfile: {e}"))?;
    write_file_atomic(&path, format!("{json}\n").as_bytes())
}

fn remove_plugins_lockfile(repo_root: &Path) -> Result<(), String> {
    let path = plugins_lockfile_path(repo_root);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("failed to remove {}: {e}", path.display()))?;
    }
    Ok(())
}

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_csv_flag(args: &[String], flag: &str) -> Result<Option<Vec<String>>, String> {
    let mut seen = false;
    let mut out: Vec<String> = vec![];
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == flag {
            seen = true;
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("{flag} requires a value"))?;
            if value.starts_with("--") {
                return Err(format!("{flag} requires a value"));
            }
            out.extend(parse_csv(value));
            i += 2;
            continue;
        }
        i += 1;
    }
    if seen {
        Ok(Some(dedupe_strings(out)))
    } else {
        Ok(None)
    }
}

fn parse_bool_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn action_requires_admin_lane(action: PluginsAction) -> bool {
    matches!(
        action,
        PluginsAction::Install | PluginsAction::Update | PluginsAction::Uninstall
    )
}

fn ensure_admin_lane(action: PluginsAction, args: &[String]) -> Result<(), String> {
    if action_requires_admin_lane(action) && !parse_bool_flag(args, "--admin-lane") {
        return Err(format!(
            "plugins {} requires explicit --admin-lane (fail-closed)",
            match action {
                PluginsAction::Install => "install",
                PluginsAction::Update => "update",
                PluginsAction::Uninstall => "uninstall",
                PluginsAction::List => "list",
                PluginsAction::Packs => "packs",
                PluginsAction::Info => "info",
                PluginsAction::Doctor => "doctor",
            }
        ));
    }
    Ok(())
}

fn with_csv_flag(args: &mut Vec<String>, flag: &str, values: &[String]) {
    if values.is_empty() || args.iter().any(|a| a == flag) {
        return;
    }
    args.push(flag.to_string());
    args.push(values.join(","));
}

fn safe_relative_path(raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if path.as_os_str().is_empty() {
        return Err("empty relative path in registry payload".to_string());
    }
    if path.is_absolute() {
        return Err(format!(
            "absolute paths are forbidden in registry payload: {raw}"
        ));
    }
    for component in path.components() {
        match component {
            Component::CurDir | Component::Normal(_) => {}
            _ => return Err(format!("unsafe path component in registry payload: {raw}")),
        }
    }
    Ok(path)
}

fn rollback_moved_paths(moved: &[(PathBuf, PathBuf)], repo_root: &Path) {
    for (src, backup) in moved.iter().rev() {
        if !backup.exists() {
            continue;
        }
        if src.exists() {
            if src.is_file() {
                let _ = fs::remove_file(src);
            } else {
                let _ = fs::remove_dir_all(src);
            }
        }
        if let Some(parent) = src.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(backup, src);
        prune_empty_parent_dirs(backup, repo_root);
    }
}

fn prune_empty_parent_dirs(path: &Path, repo_root: &Path) {
    let mut cur = path.parent().map(PathBuf::from);
    while let Some(dir) = cur {
        if dir == repo_root || !dir.starts_with(repo_root) {
            break;
        }
        match fs::remove_dir(&dir) {
            Ok(()) => cur = dir.parent().map(PathBuf::from),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
}

include!("cache/manifest_helpers.inc.rs");

fn copy_dir_recursive_filtered(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in WalkDir::new(src) {
        let entry = entry.map_err(|e| format!("failed to walk {}: {e}", src.display()))?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            return Err(format!(
                "symlink entries are forbidden inside plugin packages: {}",
                path.display()
            ));
        }
        let rel = path
            .strip_prefix(src)
            .map_err(|e| format!("failed to relativize {}: {e}", path.display()))?;
        let target = dst.join(rel);
        let parts: BTreeSet<String> = target
            .components()
            .filter_map(|c| match c {
                Component::Normal(v) => Some(v.to_string_lossy().to_string()),
                _ => None,
            })
            .collect();
        if parts.contains("__pycache__") || parts.contains(".pytest_cache") {
            continue;
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("failed to create dir {}: {e}", target.display()))?;
            continue;
        }
        if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("failed to create parent dir {}: {e}", parent.display())
                })?;
            }
            fs::copy(path, &target).map_err(|e| {
                format!(
                    "failed to copy plugin file {} -> {}: {e}",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

#[path = "cache/ops.rs"]
mod ops;

pub(super) use ops::run_plugins_cli;
