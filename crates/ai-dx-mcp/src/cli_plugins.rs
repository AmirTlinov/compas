use super::{PluginsAction, PluginsCli};
use base64::{Engine as _, engine::general_purpose};
use fs4::fs_std::FileExt;
use p256::ecdsa::{Signature as P256Signature, VerifyingKey, signature::Verifier};
use p256::pkcs8::DecodePublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    time::SystemTime,
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

const REGISTRY_STATE_REL_PATH: &str = ".agents/mcp/compas/plugins/.registry_state.json";
const PLUGINS_LOCKFILE_REL_PATH: &str = ".agents/mcp/compas/plugins.lock.json";
const PLUGINS_LOCK_REL_PATH: &str = ".agents/mcp/compas/plugins.lock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryState {
    #[serde(default)]
    registry: String,
    #[serde(default)]
    source_root: String,
    #[serde(default)]
    plugins: Vec<String>,
    #[serde(default)]
    packs: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryManifestV1 {
    schema: String,
    registry_version: String,
    archive: RegistryArchiveV1,
    plugins: Vec<RegistryPluginV1>,
    packs: Vec<RegistryPackV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryArchiveV1 {
    name: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryPluginV1 {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    path: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    description: String,
    package: RegistryPluginPackageV1,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    maintainers: Option<Vec<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    compat: Option<serde_json::Value>,
    #[serde(default)]
    deprecated: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryPluginPackageV1 {
    version: String,
    #[serde(rename = "type")]
    kind: String,
    maturity: String,
    runtime: String,
    portable: bool,
    #[serde(default)]
    languages: Vec<String>,
    entrypoint: String,
    license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryPackV1 {
    id: String,
    description: String,
    plugins: Vec<String>,
}

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

fn is_http_url(raw: &str) -> bool {
    raw.starts_with("https://") || raw.starts_with("http://")
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

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_compas_id(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 2 || s.len() > 64 {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'a'..='z' | '0'..='9') {
        return false;
    }
    for c in chars {
        if !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') {
            return false;
        }
    }
    true
}

fn validate_manifest_v1(manifest: &RegistryManifestV1) -> Result<(), String> {
    if manifest.schema != "compas.registry.manifest.v1" {
        return Err(format!(
            "unsupported registry manifest schema: {}",
            manifest.schema
        ));
    }
    if manifest.registry_version.trim().is_empty() {
        return Err("registry manifest has empty registry_version".to_string());
    }
    if manifest.archive.name.trim().is_empty()
        || manifest.archive.name.contains('/')
        || manifest.archive.name.contains('\\')
    {
        return Err(format!(
            "invalid manifest archive.name (must be a file name): {}",
            manifest.archive.name
        ));
    }
    if !is_sha256_hex(&manifest.archive.sha256) {
        return Err(format!(
            "invalid manifest archive.sha256 (expected 64 lowercase hex chars): {}",
            manifest.archive.sha256
        ));
    }
    if manifest.plugins.is_empty() {
        return Err("registry manifest has empty plugins list".to_string());
    }
    if manifest.packs.is_empty() {
        return Err("registry manifest has empty packs list".to_string());
    }

    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut aliases: BTreeSet<String> = BTreeSet::new();
    for plugin in &manifest.plugins {
        if !is_compas_id(&plugin.id) {
            return Err(format!("invalid plugin id in manifest: {}", plugin.id));
        }
        if !ids.insert(plugin.id.clone()) {
            return Err(format!("duplicate plugin id in manifest: {}", plugin.id));
        }
        for alias in &plugin.aliases {
            if !is_compas_id(alias) {
                return Err(format!("plugin {} has invalid alias: {}", plugin.id, alias));
            }
            if ids.contains(alias) {
                return Err(format!(
                    "plugin {} alias collides with canonical plugin id: {}",
                    plugin.id, alias
                ));
            }
            if !aliases.insert(alias.clone()) {
                return Err(format!("duplicate alias in manifest: {}", alias));
            }
        }
        let plugin_path = Path::new(&plugin.path);
        if plugin_path.as_os_str().is_empty() || plugin_path.is_absolute() {
            return Err(format!(
                "plugin {} has unsafe path: {}",
                plugin.id, plugin.path
            ));
        }
        if plugin.path.contains('\\') {
            return Err(format!(
                "plugin {} has unsafe path (backslashes forbidden): {}",
                plugin.id, plugin.path
            ));
        }
        for c in plugin_path.components() {
            match c {
                Component::CurDir | Component::Normal(_) => {}
                _ => {
                    return Err(format!(
                        "plugin {} has unsafe path: {}",
                        plugin.id, plugin.path
                    ));
                }
            }
        }
        if plugin.package.version.trim().is_empty() {
            return Err(format!("plugin {} has empty package.version", plugin.id));
        }
        if plugin.package.entrypoint.trim().is_empty() {
            return Err(format!("plugin {} has empty package.entrypoint", plugin.id));
        }
    }

    for pack in &manifest.packs {
        if !is_compas_id(&pack.id) {
            return Err(format!("invalid pack id in manifest: {}", pack.id));
        }
        if pack.description.trim().len() < 8 {
            return Err(format!("pack {} description too short", pack.id));
        }
        if pack.plugins.is_empty() {
            return Err(format!("pack {} has empty plugins list", pack.id));
        }
        for plugin_id in &pack.plugins {
            if !ids.contains(plugin_id) {
                return Err(format!(
                    "pack {} references unknown plugin id: {}",
                    pack.id, plugin_id
                ));
            }
        }
    }

    Ok(())
}

const OFFICIAL_REGISTRY_COSIGN_PUBKEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAExWXyUnb9j+0nAopQJWPU2JObKitu\nfNacvZOK6C4P/AeUOQc0PmK3rSrm/NRII6pCRssOC65QTbt+0zi0dzySwQ==\n-----END PUBLIC KEY-----\n";

fn verify_cosign_blob_signature(
    manifest_bytes: &[u8],
    signature_b64: &str,
    pubkey_pem: &str,
) -> Result<String, String> {
    let key = VerifyingKey::from_public_key_pem(pubkey_pem)
        .map_err(|e| format!("failed to parse cosign public key PEM: {e}"))?;
    let sig_raw = signature_b64.trim();
    if sig_raw.is_empty() {
        return Err("empty signature".to_string());
    }
    let sig_bytes = general_purpose::STANDARD
        .decode(sig_raw)
        .map_err(|e| format!("failed to base64-decode signature: {e}"))?;
    let sig = P256Signature::from_der(&sig_bytes)
        .map_err(|e| format!("failed to parse DER ECDSA signature: {e}"))?;
    key.verify(manifest_bytes, &sig)
        .map_err(|e| format!("invalid manifest signature: {e}"))?;

    let key_id = sha256_hex(key.to_encoded_point(false).as_bytes());
    Ok(key_id)
}

fn source_key_for_local_path(path: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|e| format!("failed to resolve registry path {}: {e}", path.display()))?;
    let meta = fs::metadata(&canonical)
        .map_err(|e| format!("failed to stat registry path {}: {e}", canonical.display()))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0u64, |d| d.as_secs());
    let kind = if meta.is_dir() { "dir" } else { "file" };
    let signature = format!(
        "local|{}|{}|{}|{}",
        canonical.display(),
        kind,
        meta.len(),
        modified
    );
    Ok(sha256_hex(signature.as_bytes()))
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

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in WalkDir::new(src) {
        let entry = entry.map_err(|e| format!("failed to walk {}: {e}", src.display()))?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            return Err(format!(
                "symlink entries are forbidden in registry sources: {}",
                path.display()
            ));
        }
        let rel = path
            .strip_prefix(src)
            .map_err(|e| format!("failed to relativize {}: {e}", path.display()))?;
        let target = dst.join(rel);
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
                    "failed to copy registry file {} -> {}: {e}",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn is_gzip_archive(path: &Path) -> Result<bool, String> {
    let mut file = fs::File::open(path)
        .map_err(|e| format!("failed to open archive {}: {e}", path.display()))?;
    let mut magic = [0u8; 2];
    let read_n = file
        .read(&mut magic)
        .map_err(|e| format!("failed to read archive {}: {e}", path.display()))?;
    Ok(read_n == 2 && magic == [0x1f, 0x8b])
}

fn extract_archive(archive_path: &Path, target_dir: &Path) -> Result<(), String> {
    if !is_gzip_archive(archive_path)? {
        return Err(format!(
            "unsupported legacy registry archive format: {} (expected tar.gz)",
            archive_path.display()
        ));
    }
    let _ = extract_tar_gz_safe(archive_path, target_dir)?;
    Ok(())
}

fn locate_registry_root(base: &Path) -> Result<PathBuf, String> {
    let direct = base.join("scripts").join("compas_plugins.py");
    if direct.is_file() {
        return Ok(base.to_path_buf());
    }
    let entries = fs::read_dir(base)
        .map_err(|e| format!("failed to read cache registry dir {}: {e}", base.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("failed to read entry in {}: {e}", base.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let installer = path.join("scripts").join("compas_plugins.py");
        if installer.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "compas_plugins.py not found under {}",
        base.display()
    ))
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

fn installer_python() -> OsString {
    std::env::var_os("PYTHON")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OsString::from("python3"))
}

fn mark_ready(entry: &Path) -> Result<(), String> {
    fs::write(entry.join(".ready"), b"ok\n")
        .map_err(|e| format!("failed to write cache marker in {}: {e}", entry.display()))
}

async fn cache_registry_source(registry_source: &str) -> Result<PathBuf, String> {
    let cache_root = plugins_cache_root();
    fs::create_dir_all(&cache_root)
        .map_err(|e| format!("failed to create cache root {}: {e}", cache_root.display()))?;

    if is_http_url(registry_source) {
        let entry = cache_root.join(sha256_hex(registry_source.as_bytes()));
        let extract_dir = entry.join("extract");
        if entry.join(".ready").is_file() {
            if let Ok(root) = locate_registry_root(&extract_dir) {
                return Ok(root);
            }
        }
        ensure_clean_dir(&entry)?;
        let archive_path = entry.join("registry.tar");
        download_url_to_file(registry_source, &archive_path).await?;
        extract_archive(&archive_path, &extract_dir)?;
        mark_ready(&entry)?;
        return locate_registry_root(&extract_dir);
    }

    let source_path = PathBuf::from(registry_source);
    let source_path = fs::canonicalize(&source_path).map_err(|e| {
        format!(
            "failed to resolve registry source {}: {e}",
            source_path.display()
        )
    })?;
    let key = source_key_for_local_path(&source_path)?;
    let entry = cache_root.join(key);
    let source_cache = entry.join("source");
    if entry.join(".ready").is_file() {
        if let Ok(root) = locate_registry_root(&source_cache) {
            return Ok(root);
        }
    }

    ensure_clean_dir(&entry)?;
    if source_path.is_dir() {
        fs::create_dir_all(&source_cache).map_err(|e| {
            format!(
                "failed to create source cache {}: {e}",
                source_cache.display()
            )
        })?;
        copy_dir_recursive(&source_path, &source_cache)?;
    } else if source_path.is_file() {
        let archive_copy = entry.join(
            source_path
                .file_name()
                .map_or_else(|| OsString::from("registry.tar"), |v| v.to_os_string()),
        );
        fs::copy(&source_path, &archive_copy).map_err(|e| {
            format!(
                "failed to cache registry archive {} -> {}: {e}",
                source_path.display(),
                archive_copy.display()
            )
        })?;
        fs::create_dir_all(&source_cache).map_err(|e| {
            format!(
                "failed to create source cache {}: {e}",
                source_cache.display()
            )
        })?;
        extract_archive(&archive_copy, &source_cache)?;
    } else {
        return Err(format!(
            "registry source does not exist: {}",
            source_path.display()
        ));
    }

    mark_ready(&entry)?;
    locate_registry_root(&source_cache)
}

fn run_installer_status(
    installer: &Path,
    repo_root: &Path,
    subcommand: &str,
    args: &[String],
) -> Result<i32, String> {
    let mut command = Command::new(installer_python());
    let status = command
        .current_dir(repo_root)
        .arg(installer)
        .arg(subcommand)
        .args(args)
        .status()
        .map_err(|e| format!("failed to run registry installer: {e}"))?;
    Ok(status.code().unwrap_or(1))
}

fn run_installer_capture(
    installer: &Path,
    repo_root: &Path,
    subcommand: &str,
    args: &[String],
) -> Result<std::process::Output, String> {
    let mut command = Command::new(installer_python());
    command
        .current_dir(repo_root)
        .arg(installer)
        .arg(subcommand)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run registry installer {subcommand}: {e}"))
}

fn run_installer_json(
    installer: &Path,
    repo_root: &Path,
    subcommand: &str,
    args: &[String],
) -> Result<serde_json::Value, String> {
    let out = run_installer_capture(installer, repo_root, subcommand, args)?;
    if !out.status.success() {
        return Err(format!(
            "registry installer {subcommand} failed (exit={:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("failed to parse installer {subcommand} JSON output: {e}"))
}

fn registry_state_path(repo_root: &Path) -> PathBuf {
    repo_root.join(REGISTRY_STATE_REL_PATH)
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

fn read_registry_state(repo_root: &Path) -> Result<Option<RegistryState>, String> {
    let path = registry_state_path(repo_root);
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut parsed: RegistryState = serde_json::from_str(&raw)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    parsed.plugins = dedupe_strings(parsed.plugins);
    parsed.packs = dedupe_strings(parsed.packs);
    parsed.files = dedupe_strings(parsed.files);
    Ok(Some(parsed))
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

fn write_registry_state(repo_root: &Path, state: &RegistryState) -> Result<(), String> {
    let path = registry_state_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("failed to serialize registry state: {e}"))?;
    fs::write(&path, format!("{json}\n"))
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
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

fn remove_registry_state(repo_root: &Path) -> Result<(), String> {
    let path = registry_state_path(repo_root);
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

fn strip_manifest_only_flags(args: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = vec![];
    let mut i = 0usize;
    while i < args.len() {
        let cur = &args[i];
        match cur.as_str() {
            "--allow-unsigned" | "--allow-experimental" | "--allow-deprecated" => {
                i += 1;
            }
            "--pubkey" => {
                let _ = args
                    .get(i + 1)
                    .ok_or_else(|| "--pubkey requires a value".to_string())?;
                i += 2;
            }
            _ => {
                out.push(cur.clone());
                i += 1;
            }
        }
    }
    Ok(out)
}

fn with_csv_flag(args: &mut Vec<String>, flag: &str, values: &[String]) {
    if values.is_empty() || args.iter().any(|a| a == flag) {
        return;
    }
    args.push(flag.to_string());
    args.push(values.join(","));
}

fn parse_registry_plugin_ids(payload: &serde_json::Value) -> Result<Vec<String>, String> {
    let rows = payload
        .as_array()
        .ok_or_else(|| "registry list payload is not an array".to_string())?;
    let mut out: Vec<String> = vec![];
    for row in rows {
        if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
            out.push(id.to_string());
        }
    }
    Ok(dedupe_strings(out))
}

fn parse_pack_plugin_ids(payload: &serde_json::Value, packs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let packs_set: BTreeSet<&str> = packs.iter().map(String::as_str).collect();
    if let Some(rows) = payload.as_array() {
        for row in rows {
            let Some(pack_id) = row.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if !packs_set.contains(pack_id) {
                continue;
            }
            if let Some(items) = row.get("plugins").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(id) = item.as_str() {
                        out.push(id.to_string());
                    }
                }
            }
        }
    }
    dedupe_strings(out)
}

fn parse_install_files(payload: &serde_json::Value) -> Result<Vec<String>, String> {
    let rows = payload
        .get("install_files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "registry info payload missing install_files[]".to_string())?;
    let mut files: Vec<String> = vec![];
    for row in rows {
        if let Some(path) = row.as_str() {
            files.push(path.to_string());
        }
    }
    Ok(dedupe_strings(files))
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

fn run_plugins_update(installer: &Path, parsed: &PluginsCli) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let mut installer_args = parsed.installer_args.clone();
    let explicit_plugins = parse_csv_flag(&installer_args, "--plugins")?;
    let explicit_packs = parse_csv_flag(&installer_args, "--packs")?;

    if explicit_plugins.is_none() && explicit_packs.is_none() {
        let state = read_registry_state(&repo_root)?;
        let state = state.ok_or_else(|| {
            format!(
                "plugins update requires --plugins/--packs or existing state at {}",
                registry_state_path(&repo_root).display()
            )
        })?;
        with_csv_flag(&mut installer_args, "--plugins", &state.plugins);
        with_csv_flag(&mut installer_args, "--packs", &state.packs);
        if parse_csv_flag(&installer_args, "--plugins")?.is_none()
            && parse_csv_flag(&installer_args, "--packs")?.is_none()
        {
            return Err(
                "plugins update cannot infer targets from empty registry state; pass --plugins or --packs"
                    .to_string(),
            );
        }
    }

    run_installer_status(installer, &repo_root, "install", &installer_args)
}

fn run_plugins_uninstall(installer: &Path, parsed: &PluginsCli) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let state = read_registry_state(&repo_root)?;
    let state_plugins = state
        .as_ref()
        .map(|s| s.plugins.clone())
        .unwrap_or_default();
    let mut selected_plugins =
        parse_csv_flag(&parsed.installer_args, "--plugins")?.unwrap_or_default();
    let selected_packs = parse_csv_flag(&parsed.installer_args, "--packs")?.unwrap_or_default();

    if !selected_packs.is_empty() {
        let packs_payload =
            run_installer_json(installer, &repo_root, "packs", &[String::from("--json")])?;
        selected_plugins.extend(parse_pack_plugin_ids(&packs_payload, &selected_packs));
    }
    if selected_plugins.is_empty() {
        selected_plugins.extend(state_plugins);
    }
    selected_plugins = dedupe_strings(selected_plugins);
    if selected_plugins.is_empty() {
        return Err(
            "plugins uninstall requires --plugins/--packs or existing registry state".to_string(),
        );
    }

    let dry_run = parse_bool_flag(&parsed.installer_args, "--dry-run");
    let mut files_to_remove: Vec<String> = vec![];
    for plugin_id in &selected_plugins {
        let payload = run_installer_json(
            installer,
            &repo_root,
            "info",
            std::slice::from_ref(plugin_id),
        )?;
        files_to_remove.extend(parse_install_files(&payload)?);
    }
    files_to_remove = dedupe_strings(files_to_remove);

    let mut removed_files: Vec<String> = vec![];
    let mut missing_files: Vec<String> = vec![];

    if !dry_run {
        for rel in &files_to_remove {
            let rel_path = safe_relative_path(rel)?;
            let abs = repo_root.join(&rel_path);
            if abs.is_file() {
                fs::remove_file(&abs)
                    .map_err(|e| format!("failed to remove file {}: {e}", abs.display()))?;
                prune_empty_parent_dirs(&abs, &repo_root);
                removed_files.push(rel.clone());
                continue;
            }
            if abs.is_dir() {
                fs::remove_dir_all(&abs)
                    .map_err(|e| format!("failed to remove directory {}: {e}", abs.display()))?;
                prune_empty_parent_dirs(&abs, &repo_root);
                removed_files.push(rel.clone());
                continue;
            }
            missing_files.push(rel.clone());
        }
    }

    let mut state_updated = false;
    if !dry_run && let Some(mut state_payload) = state {
        let plugins_set: BTreeSet<String> = selected_plugins.iter().cloned().collect();
        let packs_set: BTreeSet<String> = selected_packs.iter().cloned().collect();
        let files_set: BTreeSet<String> = files_to_remove.iter().cloned().collect();
        state_payload.plugins.retain(|p| !plugins_set.contains(p));
        state_payload.packs.retain(|p| !packs_set.contains(p));
        state_payload.files.retain(|p| !files_set.contains(p));
        state_payload.plugins = dedupe_strings(state_payload.plugins);
        state_payload.packs = dedupe_strings(state_payload.packs);
        state_payload.files = dedupe_strings(state_payload.files);

        if state_payload.plugins.is_empty()
            && state_payload.packs.is_empty()
            && state_payload.files.is_empty()
        {
            remove_registry_state(&repo_root)?;
        } else {
            write_registry_state(&repo_root, &state_payload)?;
        }
        state_updated = true;
    }

    let payload = serde_json::json!({
        "ok": true,
        "dry_run": dry_run,
        "repo_root": repo_root,
        "plugins": selected_plugins,
        "packs": selected_packs,
        "file_count": files_to_remove.len(),
        "planned_files": files_to_remove,
        "removed_files": removed_files,
        "missing_files": missing_files,
        "state_path": registry_state_path(&repo_root),
        "state_updated": state_updated,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize uninstall summary: {e}"))?
    );
    Ok(0)
}

fn run_plugins_doctor(installer: &Path, parsed: &PluginsCli) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let list_payload =
        run_installer_json(installer, &repo_root, "list", &[String::from("--json")])?;
    let registry_plugins = parse_registry_plugin_ids(&list_payload)?;
    let registry_set: BTreeSet<String> = registry_plugins.iter().cloned().collect();
    let state = read_registry_state(&repo_root)?;
    let mut state_plugins: Vec<String> = vec![];
    let mut state_files: Vec<String> = vec![];
    if let Some(s) = &state {
        state_plugins = s.plugins.clone();
        state_files = s.files.clone();
    }

    let mut unknown_state_plugins: Vec<String> = vec![];
    for plugin in &state_plugins {
        if !registry_set.contains(plugin) {
            unknown_state_plugins.push(plugin.clone());
        }
    }

    let mut missing_files: Vec<String> = vec![];
    let mut invalid_paths: Vec<String> = vec![];
    for rel in &state_files {
        let rel_path = match safe_relative_path(rel) {
            Ok(p) => p,
            Err(_) => {
                invalid_paths.push(rel.clone());
                continue;
            }
        };
        if !repo_root.join(rel_path).exists() {
            missing_files.push(rel.clone());
        }
    }

    let ok =
        unknown_state_plugins.is_empty() && missing_files.is_empty() && invalid_paths.is_empty();
    let payload = serde_json::json!({
        "ok": ok,
        "repo_root": repo_root,
        "state_path": registry_state_path(&repo_root),
        "state_present": state.is_some(),
        "registry_plugins_total": registry_plugins.len(),
        "state_plugins": state_plugins,
        "unknown_state_plugins": unknown_state_plugins,
        "missing_files": missing_files,
        "invalid_state_paths": invalid_paths,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize doctor summary: {e}"))?
    );
    Ok(if ok { 0 } else { 1 })
}

#[derive(Debug, Clone)]
struct ManifestResolved {
    manifest: RegistryManifestV1,
    manifest_sha256: String,
    signature_key_id: Option<String>,
    base_url: Option<String>,
    base_dir: Option<PathBuf>,
}

fn looks_like_manifest_source(registry_source: &str) -> Result<bool, String> {
    if is_http_url(registry_source) {
        return Ok(registry_source.ends_with(".json"));
    }
    let path = PathBuf::from(registry_source);
    if path.is_file() && registry_source.ends_with(".json") {
        return Ok(true);
    }
    if path.is_file() {
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let first = raw.chars().find(|c| !c.is_whitespace());
        return Ok(first == Some('{'));
    }
    Ok(false)
}

fn parse_string_flag(args: &[String], flag: &str) -> Result<Option<String>, String> {
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == flag {
            let v = args
                .get(i + 1)
                .ok_or_else(|| format!("{flag} requires a value"))?;
            if v.starts_with("--") {
                return Err(format!("{flag} requires a value"));
            }
            return Ok(Some(v.clone()));
        }
        i += 1;
    }
    Ok(None)
}

fn normalize_plugin_inputs(inputs: Vec<String>) -> Vec<String> {
    inputs
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn manifest_alias_map(manifest: &RegistryManifestV1) -> BTreeSet<(String, String)> {
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for plugin in &manifest.plugins {
        for alias in &plugin.aliases {
            out.insert((alias.clone(), plugin.id.clone()));
        }
    }
    out
}

fn resolve_plugin_ids_from_manifest(
    manifest: &RegistryManifestV1,
    plugin_inputs: &[String],
    pack_inputs: &[String],
) -> Result<Vec<String>, String> {
    let mut by_id: BTreeSet<String> = BTreeSet::new();
    for p in &manifest.plugins {
        by_id.insert(p.id.clone());
    }
    let mut alias_to_id: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for (alias, pid) in manifest_alias_map(manifest) {
        alias_to_id.insert(alias, pid);
    }

    let mut packs_by_id: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for pack in &manifest.packs {
        packs_by_id.insert(pack.id.clone(), pack.plugins.clone());
    }

    let mut unknown_packs: Vec<String> = vec![];
    let mut expanded_inputs: Vec<String> = vec![];
    for pack_id in pack_inputs {
        if let Some(items) = packs_by_id.get(pack_id) {
            expanded_inputs.extend(items.clone());
        } else {
            unknown_packs.push(pack_id.clone());
        }
    }
    if !unknown_packs.is_empty() {
        return Err(format!("unknown packs: {}", unknown_packs.join(", ")));
    }
    expanded_inputs.extend_from_slice(plugin_inputs);

    let mut unknown_plugins: Vec<String> = vec![];
    let mut resolved: Vec<String> = vec![];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for raw in expanded_inputs {
        let mut pid = raw.clone();
        if !by_id.contains(&pid) {
            if let Some(mapped) = alias_to_id.get(&pid) {
                pid = mapped.clone();
            }
        }
        if !by_id.contains(&pid) {
            unknown_plugins.push(raw);
            continue;
        }
        if seen.insert(pid.clone()) {
            resolved.push(pid);
        }
    }
    if !unknown_plugins.is_empty() {
        return Err(format!("unknown plugins: {}", unknown_plugins.join(", ")));
    }
    resolved.sort();
    Ok(resolved)
}

fn extract_base_url(url: &str) -> Option<String> {
    let (base, _tail) = url.rsplit_once('/')?;
    Some(base.to_string())
}

fn signature_source_for_manifest_source(source: &str) -> String {
    format!("{source}.sig")
}

#[cfg(feature = "full")]
async fn fetch_url_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to download {url}: {e}"))?;
    let response = response
        .error_for_status()
        .map_err(|e| format!("download failed for {url}: {e}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read body from {url}: {e}"))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "downloaded payload too large from {url}: {} bytes (max {max_bytes})",
            bytes.len()
        ));
    }
    Ok(bytes.to_vec())
}

#[cfg(not(feature = "full"))]
async fn fetch_url_bytes(url: &str, _max_bytes: usize) -> Result<Vec<u8>, String> {
    Err(format!(
        "URL registry sources are unavailable in lite build ({url}); use local --registry path"
    ))
}

async fn load_verified_manifest(parsed: &PluginsCli) -> Result<ManifestResolved, String> {
    let allow_unsigned = parse_bool_flag(&parsed.installer_args, "--allow-unsigned");
    let pubkey_override = parse_string_flag(&parsed.installer_args, "--pubkey")?;

    let registry_source = parsed.registry_source.trim().to_string();
    let manifest_bytes: Vec<u8>;
    let mut signature_b64: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut base_dir: Option<PathBuf> = None;

    if is_http_url(&registry_source) {
        manifest_bytes = fetch_url_bytes(&registry_source, 5 * 1024 * 1024).await?;
        if !allow_unsigned {
            let sig_url = signature_source_for_manifest_source(&registry_source);
            let sig_bytes = fetch_url_bytes(&sig_url, 512 * 1024).await?;
            signature_b64 = Some(
                String::from_utf8(sig_bytes)
                    .map_err(|e| format!("signature is not valid UTF-8: {e}"))?,
            );
        }
        base_url = extract_base_url(&registry_source);
    } else {
        let path = PathBuf::from(&registry_source);
        let path = fs::canonicalize(&path)
            .map_err(|e| format!("failed to resolve registry source {}: {e}", path.display()))?;
        manifest_bytes = fs::read(&path)
            .map_err(|e| format!("failed to read manifest {}: {e}", path.display()))?;
        let sig_path = path.with_extension(format!(
            "{}.sig",
            path.extension().and_then(|s| s.to_str()).unwrap_or("json")
        ));
        if sig_path.is_file() {
            signature_b64 =
                Some(fs::read_to_string(&sig_path).map_err(|e| {
                    format!("failed to read signature {}: {e}", sig_path.display())
                })?);
        }
        base_dir = path.parent().map(PathBuf::from);
    }

    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest: RegistryManifestV1 = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("failed to parse registry manifest JSON: {e}"))?;
    validate_manifest_v1(&manifest)?;

    let pubkey_pem = if let Some(path) = pubkey_override {
        fs::read_to_string(&path).map_err(|e| format!("failed to read pubkey {}: {e}", path))?
    } else {
        OFFICIAL_REGISTRY_COSIGN_PUBKEY_PEM.to_string()
    };

    let signature_key_id = if allow_unsigned {
        None
    } else {
        let sig = signature_b64.as_deref().ok_or_else(|| {
            "missing registry manifest signature (.sig); use --allow-unsigned to bypass".to_string()
        })?;
        Some(verify_cosign_blob_signature(
            &manifest_bytes,
            sig,
            &pubkey_pem,
        )?)
    };

    Ok(ManifestResolved {
        manifest,
        manifest_sha256,
        signature_key_id,
        base_url,
        base_dir,
    })
}

fn registry_cache_root_for_manifest(resolved: &ManifestResolved) -> PathBuf {
    plugins_cache_root()
        .join("manifest-v1")
        .join(resolved.manifest_sha256.clone())
}

fn locate_single_dir(path: &Path) -> Result<PathBuf, String> {
    let entries =
        fs::read_dir(path).map_err(|e| format!("failed to read dir {}: {e}", path.display()))?;
    let mut dirs: Vec<PathBuf> = vec![];
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("failed to read entry in {}: {e}", path.display()))?;
        let p = entry.path();
        if p.is_dir() {
            dirs.push(p);
        }
    }
    dirs.sort();
    if dirs.len() != 1 {
        return Err(format!(
            "expected exactly one top-level directory under {}, found {}",
            path.display(),
            dirs.len()
        ));
    }
    Ok(dirs.remove(0))
}

#[cfg(feature = "full")]
fn extract_tar_gz_safe(archive_path: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    const MAX_ENTRIES: usize = 20_000;
    const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
    const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
    const MAX_PATH_BYTES: usize = 512;

    fs::create_dir_all(out_dir)
        .map_err(|e| format!("failed to create extract dir {}: {e}", out_dir.display()))?;

    let file = fs::File::open(archive_path)
        .map_err(|e| format!("failed to open archive {}: {e}", archive_path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let mut root_prefix: Option<String> = None;
    let mut entries_seen: usize = 0;
    let mut total_bytes: u64 = 0;

    for entry in archive
        .entries()
        .map_err(|e| format!("failed to read tar entries: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("failed to read tar entry: {e}"))?;
        entries_seen += 1;
        if entries_seen > MAX_ENTRIES {
            return Err(format!("archive exceeds MAX_ENTRIES={MAX_ENTRIES}"));
        }

        let entry_type = entry.header().entry_type();
        if entry_type != tar::EntryType::Regular && entry_type != tar::EntryType::Directory {
            return Err(format!("unsupported tar entry type: {entry_type:?}"));
        }

        let path = entry
            .path()
            .map_err(|e| format!("failed to read tar entry path: {e}"))?
            .into_owned();
        if path.as_os_str().is_empty() {
            return Err("empty tar entry path".to_string());
        }
        if path.is_absolute() {
            return Err(format!(
                "absolute tar path is forbidden: {}",
                path.display()
            ));
        }

        let path_str = path.to_string_lossy();
        if path_str.as_bytes().len() > MAX_PATH_BYTES {
            return Err(format!(
                "tar path too long (> {MAX_PATH_BYTES} bytes): {path_str}"
            ));
        }

        let mut components = path.components();
        let Some(Component::Normal(first)) = components.next() else {
            return Err(format!("unsafe tar path component: {}", path.display()));
        };
        let first = first.to_string_lossy().to_string();
        if root_prefix.is_none() {
            root_prefix = Some(first.clone());
        } else if root_prefix.as_deref() != Some(first.as_str()) {
            return Err(format!(
                "archive must have single top-level directory; found '{}' and '{}'",
                root_prefix.as_deref().unwrap_or(""),
                first
            ));
        }

        for c in components.clone() {
            match c {
                Component::Normal(_) => {}
                Component::CurDir => {}
                _ => return Err(format!("unsafe tar path component: {}", path.display())),
            }
        }

        let size = entry
            .header()
            .size()
            .map_err(|e| format!("failed to read tar entry size: {e}"))?;
        if entry_type == tar::EntryType::Regular {
            if size > MAX_FILE_BYTES {
                return Err(format!(
                    "tar entry too large (> {MAX_FILE_BYTES} bytes): {}",
                    path.display()
                ));
            }
            total_bytes = total_bytes.saturating_add(size);
            if total_bytes > MAX_TOTAL_BYTES {
                return Err(format!("archive exceeds MAX_TOTAL_BYTES={MAX_TOTAL_BYTES}"));
            }
        }

        let target = out_dir.join(&path);
        if !target.starts_with(out_dir) {
            return Err(format!(
                "tar extraction escape detected: {}",
                target.display()
            ));
        }

        if entry_type == tar::EntryType::Directory {
            fs::create_dir_all(&target)
                .map_err(|e| format!("failed to create dir {}: {e}", target.display()))?;
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create parent dir {}: {e}", parent.display()))?;
        }
        entry
            .unpack(&target)
            .map_err(|e| format!("failed to unpack {}: {e}", target.display()))?;
    }

    locate_single_dir(out_dir)
}

#[cfg(not(feature = "full"))]
fn extract_tar_gz_safe(_archive_path: &Path, _out_dir: &Path) -> Result<PathBuf, String> {
    Err("archive extraction is unavailable in lite build; use full build".to_string())
}

async fn ensure_registry_cached(resolved: &ManifestResolved) -> Result<PathBuf, String> {
    let entry = registry_cache_root_for_manifest(resolved);
    let extract_dir = entry.join("extract");
    if entry.join(".ready").is_file() {
        if extract_dir.is_dir() {
            return locate_single_dir(&extract_dir);
        }
    }

    ensure_clean_dir(&entry)?;
    fs::create_dir_all(&extract_dir).map_err(|e| {
        format!(
            "failed to create extract dir {}: {e}",
            extract_dir.display()
        )
    })?;

    let archive_path = entry.join(&resolved.manifest.archive.name);

    if let Some(base_url) = &resolved.base_url {
        let url = format!("{base_url}/{}", resolved.manifest.archive.name);
        download_url_to_file(&url, &archive_path).await?;
    } else if let Some(base_dir) = &resolved.base_dir {
        let local = base_dir.join(&resolved.manifest.archive.name);
        if !local.is_file() {
            return Err(format!(
                "archive not found next to manifest: {}",
                local.display()
            ));
        }
        fs::copy(&local, &archive_path).map_err(|e| {
            format!(
                "failed to copy archive {} -> {}: {e}",
                local.display(),
                archive_path.display()
            )
        })?;
    } else {
        return Err("cannot resolve archive location for registry manifest source".to_string());
    }

    let actual_sha = sha256_file(&archive_path)?;
    if actual_sha != resolved.manifest.archive.sha256 {
        return Err(format!(
            "archive sha256 mismatch for {}: expected {}, got {}",
            archive_path.display(),
            resolved.manifest.archive.sha256,
            actual_sha
        ));
    }

    let root = extract_tar_gz_safe(&archive_path, &extract_dir)?;
    mark_ready(&entry)?;
    Ok(root)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| format!("failed to stat {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "refusing to hash symlink path (unsafe): {}",
            path.display()
        ));
    }
    if !meta.is_file() {
        return Err(format!(
            "refusing to hash non-file path: {}",
            path.display()
        ));
    }
    let mut file =
        fs::File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_repo_rel_path(repo_root: &Path, abs: &Path) -> Result<String, String> {
    let rel = abs
        .strip_prefix(repo_root)
        .map_err(|e| format!("failed to relativize {}: {e}", abs.display()))?;
    let mut s = rel.to_string_lossy().to_string();
    s = s.replace('\\', "/");
    if s.is_empty() {
        return Err("empty relative path".to_string());
    }
    Ok(s)
}

fn collect_staged_plugin_lock_entries(
    staged_plugin_root: &Path,
    plugin_id: &str,
) -> Result<Vec<PluginsLockfileEntryV1>, String> {
    let mut out: Vec<PluginsLockfileEntryV1> = vec![];
    for entry in WalkDir::new(staged_plugin_root) {
        let entry = entry.map_err(|e| {
            format!(
                "failed to walk staged plugin dir {}: {e}",
                staged_plugin_root.display()
            )
        })?;
        if entry.file_type().is_symlink() {
            return Err(format!(
                "symlink entries are forbidden inside staged plugin dirs: {}",
                entry.path().display()
            ));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(staged_plugin_root).map_err(|e| {
            format!(
                "failed to relativize staged plugin file {}: {e}",
                entry.path().display()
            )
        })?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        let sha256 = sha256_file(entry.path())?;
        out.push(PluginsLockfileEntryV1 {
            path: format!(".agents/mcp/compas/plugins/{plugin_id}/{rel}"),
            sha256,
            plugin_ids: vec![plugin_id.to_string()],
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn plugin_by_id<'a>(
    manifest: &'a RegistryManifestV1,
    plugin_id: &str,
) -> Option<&'a RegistryPluginV1> {
    manifest.plugins.iter().find(|p| p.id == plugin_id)
}

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

fn run_plugins_list_manifest(resolved: &ManifestResolved, json: bool) -> Result<i32, String> {
    let mut rows: Vec<serde_json::Value> = vec![];
    for plugin in &resolved.manifest.plugins {
        rows.push(serde_json::json!({
            "id": plugin.id,
            "aliases": plugin.aliases,
            "version": plugin.package.version,
            "tier": plugin.tier,
            "maintainers": plugin.maintainers,
            "tags": plugin.tags,
            "compat": plugin.compat,
            "deprecated": plugin.deprecated,
            "status": plugin.status,
            "description": plugin.description,
            "path": plugin.path,
        }));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|e| format!("failed to serialize plugin list: {e}"))?
        );
    } else {
        for row in &rows {
            let id = row.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let version = row.get("version").and_then(|v| v.as_str()).unwrap_or("-");
            println!("{id:<28} {version}");
        }
    }
    Ok(0)
}

fn run_plugins_packs_manifest(resolved: &ManifestResolved, json: bool) -> Result<i32, String> {
    let mut rows: Vec<serde_json::Value> = vec![];
    for pack in &resolved.manifest.packs {
        rows.push(serde_json::json!({
            "id": pack.id,
            "description": pack.description,
            "plugins": pack.plugins,
        }));
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|e| format!("failed to serialize packs list: {e}"))?
        );
    } else {
        for row in &rows {
            let id = row.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let desc = row
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            println!("{id}: {desc}");
        }
    }
    Ok(0)
}

fn run_plugins_info_manifest(resolved: &ManifestResolved, args: &[String]) -> Result<i32, String> {
    let plugin_query = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .ok_or_else(|| "plugins info requires plugin id".to_string())?;
    let mut plugin_id = plugin_query.clone();
    if plugin_by_id(&resolved.manifest, &plugin_id).is_none() {
        for plugin in &resolved.manifest.plugins {
            if plugin.aliases.iter().any(|a| a == &plugin_query) {
                plugin_id = plugin.id.clone();
                break;
            }
        }
    }
    let Some(plugin) = plugin_by_id(&resolved.manifest, &plugin_id) else {
        return Err(format!("unknown plugin: {plugin_query}"));
    };
    let payload = serde_json::json!({
        "id": plugin.id,
        "queried_as": plugin_query,
        "aliases": plugin.aliases,
        "version": plugin.package.version,
        "tier": plugin.tier,
        "maintainers": plugin.maintainers,
        "tags": plugin.tags,
        "compat": plugin.compat,
        "deprecated": plugin.deprecated,
        "status": plugin.status,
        "description": plugin.description,
        "path": plugin.path,
        "package": plugin.package,
        "registry_version": resolved.manifest.registry_version,
        "manifest_sha256": resolved.manifest_sha256,
        "signature_key_id": resolved.signature_key_id,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize plugin info: {e}"))?
    );
    Ok(0)
}

fn governance_block_reason(
    plugin: &RegistryPluginV1,
    allow_experimental: bool,
    allow_deprecated: bool,
) -> Option<(String, String)> {
    let deprecated_meta_present = plugin
        .deprecated
        .as_ref()
        .and_then(|value| value.as_object())
        .is_some_and(|obj| !obj.is_empty());
    let tier = plugin
        .tier
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match tier.as_str() {
        "experimental" if !allow_experimental => Some((
            tier,
            "install is blocked: tier=experimental (add --allow-experimental to proceed)"
                .to_string(),
        )),
        "deprecated" if !allow_deprecated => Some((
            tier,
            "install is blocked: tier=deprecated (add --allow-deprecated to proceed)".to_string(),
        )),
        _ if deprecated_meta_present && !allow_deprecated => Some((
            "deprecated".to_string(),
            "install is blocked: deprecated metadata present (add --allow-deprecated to proceed)"
                .to_string(),
        )),
        _ => None,
    }
}

async fn run_plugins_install_manifest(
    resolved: &ManifestResolved,
    parsed: &PluginsCli,
) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let _lock = acquire_plugins_op_lock(&repo_root)?;

    let plugin_inputs = parse_csv_flag(&parsed.installer_args, "--plugins")?.unwrap_or_default();
    let pack_inputs = parse_csv_flag(&parsed.installer_args, "--packs")?.unwrap_or_default();
    let dry_run = parse_bool_flag(&parsed.installer_args, "--dry-run");
    let force = parse_bool_flag(&parsed.installer_args, "--force");
    let allow_experimental = parse_bool_flag(&parsed.installer_args, "--allow-experimental");
    let allow_deprecated = parse_bool_flag(&parsed.installer_args, "--allow-deprecated");

    let plugin_inputs = normalize_plugin_inputs(plugin_inputs);
    let pack_inputs = normalize_plugin_inputs(pack_inputs);
    if plugin_inputs.is_empty() && pack_inputs.is_empty() {
        return Err("plugins install requires --plugins and/or --packs".to_string());
    }

    let plugin_ids =
        resolve_plugin_ids_from_manifest(&resolved.manifest, &plugin_inputs, &pack_inputs)?;
    let mut blocked_plugins: Vec<serde_json::Value> = vec![];
    for pid in &plugin_ids {
        let Some(plugin) = plugin_by_id(&resolved.manifest, pid) else {
            return Err(format!("plugin not found in manifest: {pid}"));
        };
        if let Some((tier, reason)) =
            governance_block_reason(plugin, allow_experimental, allow_deprecated)
        {
            blocked_plugins.push(serde_json::json!({
                "id": plugin.id,
                "tier": tier,
                "reason": reason,
            }));
        }
    }
    if !blocked_plugins.is_empty() {
        let payload = serde_json::json!({
            "ok": false,
            "dry_run": dry_run,
            "force": force,
            "blocked": true,
            "repo_root": repo_root,
            "registry_version": resolved.manifest.registry_version,
            "manifest_sha256": resolved.manifest_sha256,
            "signature_key_id": resolved.signature_key_id,
            "plugins": plugin_ids,
            "packs": pack_inputs,
            "governance": {
                "allow_experimental": allow_experimental,
                "allow_deprecated": allow_deprecated,
                "blocked_plugins": blocked_plugins,
            },
            "hint": "use --allow-experimental and/or --allow-deprecated for native registry install/update",
            "lockfile_path": plugins_lockfile_path(&repo_root),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|e| format!("failed to serialize install summary: {e}"))?
        );
        return Ok(1);
    }

    let plugin_target_set: BTreeSet<String> = plugin_ids.iter().cloned().collect();
    let plugins_root = repo_root.join(".agents/mcp/compas/plugins");
    let existing_lockfile = read_plugins_lockfile(&repo_root)?;

    let mut managed_plugin_set: BTreeSet<String> = BTreeSet::new();
    let mut managed_paths_for_targets: BTreeSet<String> = BTreeSet::new();
    if let Some(lockfile) = &existing_lockfile {
        for pid in &lockfile.plugins {
            managed_plugin_set.insert(pid.clone());
        }
        for entry in &lockfile.files {
            if entry
                .plugin_ids
                .iter()
                .any(|p| plugin_target_set.contains(p))
            {
                managed_paths_for_targets.insert(entry.path.clone());
            }
        }
    }

    let mut unmanaged_plugin_dirs: Vec<String> = vec![];
    if plugins_root.is_dir() {
        let entries = fs::read_dir(&plugins_root)
            .map_err(|e| format!("failed to read plugins dir {}: {e}", plugins_root.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|e| format!("failed to read entry in {}: {e}", plugins_root.display()))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if name == ".staging" {
                continue;
            }
            if !managed_plugin_set.contains(&name) {
                unmanaged_plugin_dirs.push(name);
            }
        }
    }
    unmanaged_plugin_dirs.sort();

    let mut missing_files: Vec<String> = vec![];
    let mut modified_files: Vec<String> = vec![];
    if let Some(lockfile) = &existing_lockfile {
        for entry in &lockfile.files {
            if !entry
                .plugin_ids
                .iter()
                .any(|p| plugin_target_set.contains(p))
            {
                continue;
            }
            let rel = safe_relative_path(&entry.path)?;
            let abs = repo_root.join(&rel);
            if !abs.exists() {
                missing_files.push(entry.path.clone());
                continue;
            }
            if !abs.is_file() {
                modified_files.push(entry.path.clone());
                continue;
            }
            let actual = sha256_file(&abs)?;
            if actual != entry.sha256 {
                modified_files.push(entry.path.clone());
            }
        }
    }
    missing_files.sort();
    modified_files.sort();

    let mut unknown_files: Vec<String> = vec![];
    if plugins_root.is_dir() {
        for pid in &plugin_ids {
            let dir = plugins_root.join(pid);
            if !dir.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&dir) {
                let entry = entry.map_err(|e| format!("failed to walk {}: {e}", dir.display()))?;
                if entry.file_type().is_symlink() {
                    unknown_files.push(normalize_repo_rel_path(&repo_root, entry.path())?);
                    continue;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let rel = normalize_repo_rel_path(&repo_root, entry.path())?;
                if !managed_paths_for_targets.contains(&rel) {
                    unknown_files.push(rel);
                }
            }
        }
    }
    unknown_files.sort();

    let blocked = !force
        && (!unmanaged_plugin_dirs.is_empty()
            || !missing_files.is_empty()
            || !modified_files.is_empty()
            || !unknown_files.is_empty());
    if blocked {
        let payload = serde_json::json!({
            "ok": false,
            "dry_run": dry_run,
            "force": force,
            "blocked": true,
            "repo_root": repo_root,
            "registry_version": resolved.manifest.registry_version,
            "manifest_sha256": resolved.manifest_sha256,
            "signature_key_id": resolved.signature_key_id,
            "plugins": plugin_ids,
            "packs": pack_inputs,
            "preflight": {
                "unmanaged_plugin_dirs": unmanaged_plugin_dirs,
                "missing_files": missing_files,
                "modified_files": modified_files,
                "unknown_files": unknown_files,
            },
            "hint": "run with --force to overwrite unmanaged/drifted plugin state",
            "lockfile_path": plugins_lockfile_path(&repo_root),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|e| format!("failed to serialize install summary: {e}"))?
        );
        return Ok(1);
    }

    let registry_root = ensure_registry_cached(resolved).await?;
    let plugins_root = repo_root.join(".agents/mcp/compas/plugins");
    let staging_root = repo_root
        .join(".agents/mcp/compas/plugins/.staging")
        .join(format!("install-{}", op_nonce()));
    let staging_plugins_root = staging_root.join("plugins");
    let staging_backups_root = staging_root.join("backups");
    fs::create_dir_all(&staging_plugins_root)
        .map_err(|e| format!("failed to create {}: {e}", staging_plugins_root.display()))?;
    fs::create_dir_all(&staging_backups_root)
        .map_err(|e| format!("failed to create {}: {e}", staging_backups_root.display()))?;

    let mut staged_lock_entries: Vec<PluginsLockfileEntryV1> = vec![];
    let mut installed: Vec<String> = vec![];
    for pid in &plugin_ids {
        let Some(plugin) = plugin_by_id(&resolved.manifest, pid) else {
            return Err(format!("plugin not found in manifest: {pid}"));
        };
        let src = registry_root.join(&plugin.path);
        if !src.is_dir() {
            return Err(format!(
                "plugin directory missing in registry cache: {}",
                src.display()
            ));
        }

        let stage_dst = staging_plugins_root.join(pid);
        installed.push(pid.clone());
        copy_dir_recursive_filtered(&src, &stage_dst)?;
        let plugin_files = collect_staged_plugin_lock_entries(&stage_dst, pid)?;
        staged_lock_entries.extend(plugin_files);
    }

    staged_lock_entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut final_lock_entries: Vec<PluginsLockfileEntryV1> = vec![];
    let mut final_plugins: Vec<String> = installed.clone();
    let mut final_packs = pack_inputs.clone();
    if let Some(existing) = existing_lockfile.clone() {
        final_plugins.extend(
            existing
                .plugins
                .into_iter()
                .filter(|p| !plugin_target_set.contains(p)),
        );
        final_packs.extend(existing.packs.into_iter());
        final_lock_entries.extend(
            existing
                .files
                .into_iter()
                .filter(|e| !e.plugin_ids.iter().any(|p| plugin_target_set.contains(p))),
        );
    }
    final_lock_entries.extend(staged_lock_entries.clone());

    let mut merged: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for entry in final_lock_entries {
        let slot = merged
            .entry(entry.path.clone())
            .or_insert_with(|| (entry.sha256.clone(), BTreeSet::new()));
        if slot.0 != entry.sha256 {
            return Err(format!(
                "conflicting hashes for managed path {} ({} vs {})",
                entry.path, slot.0, entry.sha256
            ));
        }
        for pid in entry.plugin_ids {
            slot.1.insert(pid);
        }
    }
    let mut merged_entries: Vec<PluginsLockfileEntryV1> = vec![];
    for (path, (sha256, owners)) in merged {
        merged_entries.push(PluginsLockfileEntryV1 {
            path,
            sha256,
            plugin_ids: owners.into_iter().collect(),
        });
    }
    merged_entries.sort_by(|a, b| a.path.cmp(&b.path));
    final_plugins = dedupe_strings(final_plugins);
    final_plugins.sort();
    final_packs = dedupe_strings(final_packs);
    final_packs.sort();

    if !dry_run {
        fs::create_dir_all(&plugins_root).map_err(|e| {
            format!(
                "failed to create plugins root {}: {e}",
                plugins_root.display()
            )
        })?;
        let mut swapped_plugins: Vec<String> = vec![];
        let mut backed_up_plugins: Vec<String> = vec![];
        let swap_result: Result<(), String> = (|| {
            for pid in &installed {
                let stage_dir = staging_plugins_root.join(pid);
                let dst_dir = plugins_root.join(pid);
                let backup_dir = staging_backups_root.join(pid);
                if dst_dir.exists() {
                    fs::rename(&dst_dir, &backup_dir).map_err(|e| {
                        format!(
                            "failed to move existing plugin dir {} to backup {}: {e}",
                            dst_dir.display(),
                            backup_dir.display()
                        )
                    })?;
                    backed_up_plugins.push(pid.clone());
                }
                fs::rename(&stage_dir, &dst_dir).map_err(|e| {
                    format!(
                        "failed to activate staged plugin {} -> {}: {e}",
                        stage_dir.display(),
                        dst_dir.display()
                    )
                })?;
                swapped_plugins.push(pid.clone());
            }
            Ok(())
        })();

        if let Err(swap_err) = swap_result {
            // Best-effort rollback.
            for pid in swapped_plugins.iter().rev() {
                let dst_dir = plugins_root.join(pid);
                let backup_dir = staging_backups_root.join(pid);
                if dst_dir.exists() {
                    let _ = fs::remove_dir_all(&dst_dir);
                }
                if backup_dir.exists() {
                    let _ = fs::rename(&backup_dir, &dst_dir);
                }
            }
            for pid in backed_up_plugins.iter().rev() {
                let dst_dir = plugins_root.join(pid);
                let backup_dir = staging_backups_root.join(pid);
                if !dst_dir.exists() && backup_dir.exists() {
                    let _ = fs::rename(&backup_dir, &dst_dir);
                }
            }
            let _ = fs::remove_dir_all(&staging_root);
            return Err(format!(
                "plugin install aborted; rollback executed: {swap_err}"
            ));
        }

        let lockfile = PluginsLockfileV1 {
            schema: "compas.plugins.lock.v1".to_string(),
            registry_source: parsed.registry_source.clone(),
            registry_version: resolved.manifest.registry_version.clone(),
            manifest_sha256: Some(resolved.manifest_sha256.clone()),
            signature_key_id: resolved.signature_key_id.clone(),
            plugins: final_plugins.clone(),
            packs: final_packs.clone(),
            files: merged_entries.clone(),
        };
        if let Err(lock_err) = write_plugins_lockfile(&repo_root, &lockfile) {
            for pid in installed.iter().rev() {
                let dst_dir = plugins_root.join(pid);
                let backup_dir = staging_backups_root.join(pid);
                if dst_dir.exists() {
                    let _ = fs::remove_dir_all(&dst_dir);
                }
                if backup_dir.exists() {
                    let _ = fs::rename(&backup_dir, &dst_dir);
                }
            }
            let _ = fs::remove_dir_all(&staging_root);
            return Err(format!(
                "failed to persist plugins lockfile; rollback executed: {lock_err}"
            ));
        }
    }
    let _ = fs::remove_dir_all(&staging_root);

    let payload = serde_json::json!({
        "ok": true,
        "dry_run": dry_run,
        "force": force,
        "blocked": false,
        "repo_root": repo_root,
        "registry_version": resolved.manifest.registry_version,
        "manifest_sha256": resolved.manifest_sha256,
        "signature_key_id": resolved.signature_key_id,
        "plugins": installed,
        "packs": final_packs,
        "file_count": merged_entries.len(),
        "preflight": {
            "unmanaged_plugin_dirs": unmanaged_plugin_dirs,
            "missing_files": missing_files,
            "modified_files": modified_files,
            "unknown_files": unknown_files,
        },
        "lockfile_path": plugins_lockfile_path(&repo_root),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize install summary: {e}"))?
    );
    Ok(0)
}

async fn run_plugins_update_manifest(
    resolved: &ManifestResolved,
    parsed: &PluginsCli,
) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);

    let mut plugin_inputs =
        parse_csv_flag(&parsed.installer_args, "--plugins")?.unwrap_or_default();
    let mut pack_inputs = parse_csv_flag(&parsed.installer_args, "--packs")?.unwrap_or_default();
    if plugin_inputs.is_empty() && pack_inputs.is_empty() {
        if let Some(lockfile) = read_plugins_lockfile(&repo_root)? {
            plugin_inputs = lockfile.plugins;
            pack_inputs = lockfile.packs;
        }
    }
    let mut installer_args = parsed.installer_args.clone();
    with_csv_flag(&mut installer_args, "--plugins", &plugin_inputs);
    with_csv_flag(&mut installer_args, "--packs", &pack_inputs);
    let parsed = PluginsCli {
        action: PluginsAction::Install,
        registry_source: parsed.registry_source.clone(),
        repo_root: parsed.repo_root.clone(),
        installer_args,
    };
    run_plugins_install_manifest(resolved, &parsed).await
}

fn run_plugins_uninstall_manifest(
    resolved: &ManifestResolved,
    parsed: &PluginsCli,
) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let _lock = acquire_plugins_op_lock(&repo_root)?;

    let dry_run = parse_bool_flag(&parsed.installer_args, "--dry-run");
    let force = parse_bool_flag(&parsed.installer_args, "--force");

    let plugin_inputs = parse_csv_flag(&parsed.installer_args, "--plugins")?.unwrap_or_default();
    let pack_inputs = parse_csv_flag(&parsed.installer_args, "--packs")?.unwrap_or_default();
    let plugin_inputs = normalize_plugin_inputs(plugin_inputs);
    let pack_inputs = normalize_plugin_inputs(pack_inputs);

    let lockfile = read_plugins_lockfile(&repo_root)?.ok_or_else(|| {
        format!(
            "plugins uninstall requires lockfile at {}",
            plugins_lockfile_path(&repo_root).display()
        )
    })?;

    let target_plugin_ids = if plugin_inputs.is_empty() && pack_inputs.is_empty() {
        lockfile.plugins.clone()
    } else {
        resolve_plugin_ids_from_manifest(&resolved.manifest, &plugin_inputs, &pack_inputs)?
    };
    if target_plugin_ids.is_empty() {
        return Err("no plugins selected for uninstall".to_string());
    }
    let target_set: BTreeSet<String> = target_plugin_ids.iter().cloned().collect();

    let mut planned_remove: Vec<PluginsLockfileEntryV1> = vec![];
    let mut kept_entries: Vec<PluginsLockfileEntryV1> = vec![];

    for mut entry in lockfile.files.clone() {
        let owners: BTreeSet<String> = entry.plugin_ids.iter().cloned().collect();
        let intersects = owners.iter().any(|p| target_set.contains(p));
        if !intersects {
            kept_entries.push(entry);
            continue;
        }
        entry.plugin_ids.retain(|p| !target_set.contains(p));
        if entry.plugin_ids.is_empty() {
            planned_remove.push(entry);
        } else {
            kept_entries.push(entry);
        }
    }

    planned_remove.sort_by(|a, b| a.path.cmp(&b.path));
    kept_entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut missing_files: Vec<String> = vec![];
    let mut modified_files: Vec<String> = vec![];
    let mut removed_files: Vec<String> = vec![];

    for entry in &planned_remove {
        let rel = safe_relative_path(&entry.path)?;
        let abs = repo_root.join(&rel);
        if !abs.exists() {
            missing_files.push(entry.path.clone());
            continue;
        }
        let meta = fs::symlink_metadata(&abs)
            .map_err(|e| format!("failed to stat {}: {e}", abs.display()))?;
        if meta.file_type().is_symlink() {
            modified_files.push(entry.path.clone());
            continue;
        }
        if meta.is_file() {
            let actual = sha256_file(&abs)?;
            if actual != entry.sha256 {
                modified_files.push(entry.path.clone());
            }
            continue;
        }
        // Lockfile tracks file hashes; non-file entry means type drift.
        modified_files.push(entry.path.clone());
    }
    missing_files = dedupe_strings(missing_files);
    missing_files.sort();
    modified_files = dedupe_strings(modified_files);
    modified_files.sort();

    if !modified_files.is_empty() && !force {
        let payload = serde_json::json!({
            "ok": false,
            "dry_run": dry_run,
            "repo_root": repo_root,
            "plugins": target_plugin_ids,
            "packs": pack_inputs,
            "planned_remove": planned_remove.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
            "removed_files": [],
            "missing_files": missing_files,
            "modified_files": modified_files,
            "lockfile_path": plugins_lockfile_path(&repo_root),
            "lockfile_updated": false,
            "force": force,
            "blocked": true,
            "hint": "run with --force to remove drifted paths",
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|e| format!("failed to serialize uninstall summary: {e}"))?
        );
        return Ok(1);
    }

    let mut updated = lockfile.clone();
    updated.files = kept_entries;
    if !plugin_inputs.is_empty() || !pack_inputs.is_empty() {
        updated.plugins.retain(|p| !target_set.contains(p));
        updated.packs.retain(|p| !pack_inputs.contains(p));
    } else {
        updated.plugins = vec![];
        updated.packs = vec![];
    }
    updated.plugins = dedupe_strings(updated.plugins);
    updated.packs = dedupe_strings(updated.packs);

    if !dry_run {
        let staging_root = repo_root
            .join(".agents/mcp/compas/plugins/.staging")
            .join(format!("uninstall-{}", op_nonce()));
        let backups_root = staging_root.join("backups");
        fs::create_dir_all(&backups_root)
            .map_err(|e| format!("failed to create {}: {e}", backups_root.display()))?;

        let mut moved_paths: Vec<(PathBuf, PathBuf)> = vec![];
        for entry in &planned_remove {
            let rel = safe_relative_path(&entry.path)?;
            let abs = repo_root.join(&rel);
            if !abs.exists() {
                continue;
            }
            let backup = backups_root.join(&rel);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            if backup.exists() {
                if backup.is_file() {
                    fs::remove_file(&backup)
                        .map_err(|e| format!("failed to clean backup {}: {e}", backup.display()))?;
                } else {
                    fs::remove_dir_all(&backup)
                        .map_err(|e| format!("failed to clean backup {}: {e}", backup.display()))?;
                }
            }
            if let Err(move_err) = fs::rename(&abs, &backup) {
                rollback_moved_paths(&moved_paths, &repo_root);
                let _ = fs::remove_dir_all(&staging_root);
                return Err(format!(
                    "failed to move {} to uninstall backup {}: {move_err}",
                    abs.display(),
                    backup.display()
                ));
            }
            moved_paths.push((abs.clone(), backup));
            removed_files.push(entry.path.clone());
            prune_empty_parent_dirs(&abs, &repo_root);
        }

        let commit_result: Result<(), String> = (|| {
            if std::env::var_os("COMPAS_TEST_FAIL_UNINSTALL_LOCK_COMMIT").is_some() {
                return Err("injected failure (COMPAS_TEST_FAIL_UNINSTALL_LOCK_COMMIT)".to_string());
            }
            if updated.files.is_empty() && updated.plugins.is_empty() && updated.packs.is_empty() {
                remove_plugins_lockfile(&repo_root)?;
            } else {
                write_plugins_lockfile(&repo_root, &updated)?;
            }
            Ok(())
        })();
        if let Err(commit_err) = commit_result {
            rollback_moved_paths(&moved_paths, &repo_root);
            let _ = fs::remove_dir_all(&staging_root);
            return Err(format!(
                "failed to persist uninstall lockfile transaction; rollback executed: {commit_err}"
            ));
        }
        let _ = fs::remove_dir_all(&staging_root);
    }

    removed_files = dedupe_strings(removed_files);
    removed_files.sort();
    let ok = true;
    let payload = serde_json::json!({
        "ok": ok,
        "dry_run": dry_run,
        "repo_root": repo_root,
        "plugins": target_plugin_ids,
        "packs": pack_inputs,
        "planned_remove": planned_remove.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
        "removed_files": removed_files,
        "missing_files": missing_files,
        "modified_files": modified_files,
        "lockfile_path": plugins_lockfile_path(&repo_root),
        "lockfile_updated": !dry_run,
        "force": force,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize uninstall summary: {e}"))?
    );
    Ok(if ok { 0 } else { 1 })
}

fn run_plugins_doctor_manifest(
    resolved: &ManifestResolved,
    parsed: &PluginsCli,
) -> Result<i32, String> {
    let repo_root = PathBuf::from(&parsed.repo_root);
    let lockfile = read_plugins_lockfile(&repo_root)?;
    let mut missing: Vec<String> = vec![];
    let mut modified: Vec<String> = vec![];
    let mut unknown: Vec<String> = vec![];

    let Some(lockfile) = lockfile else {
        let payload = serde_json::json!({
            "ok": false,
            "repo_root": repo_root,
            "lockfile_present": false,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(1);
    };

    let mut locked_paths: BTreeSet<String> = BTreeSet::new();
    for entry in &lockfile.files {
        locked_paths.insert(entry.path.clone());
        let rel = safe_relative_path(&entry.path)?;
        let abs = repo_root.join(rel);
        if !abs.exists() {
            missing.push(entry.path.clone());
            continue;
        }
        let meta = fs::symlink_metadata(&abs)
            .map_err(|e| format!("failed to stat {}: {e}", abs.display()))?;
        if meta.file_type().is_symlink() {
            modified.push(entry.path.clone());
            continue;
        }
        if meta.is_file() {
            let actual = sha256_file(&abs)?;
            if actual != entry.sha256 {
                modified.push(entry.path.clone());
            }
            continue;
        }
        modified.push(entry.path.clone());
    }

    let plugins_root = repo_root.join(".agents/mcp/compas/plugins");
    if plugins_root.is_dir() {
        for entry in WalkDir::new(&plugins_root) {
            let entry =
                entry.map_err(|e| format!("failed to walk {}: {e}", plugins_root.display()))?;
            let abs = entry.path().to_path_buf();
            let rel = normalize_repo_rel_path(&repo_root, &abs)?;
            if rel.starts_with(".agents/mcp/compas/plugins/.staging/") {
                continue;
            }
            if entry.file_type().is_symlink() {
                unknown.push(rel);
                continue;
            }
            if entry.file_type().is_file() && !locked_paths.contains(&rel) {
                unknown.push(rel);
            }
        }
    }

    missing.sort();
    modified.sort();
    unknown.sort();
    let ok = missing.is_empty() && modified.is_empty() && unknown.is_empty();
    let payload = serde_json::json!({
        "ok": ok,
        "repo_root": repo_root,
        "registry_source": lockfile.registry_source,
        "registry_version": lockfile.registry_version,
        "lockfile_manifest_sha256": lockfile.manifest_sha256,
        "lockfile_signature_key_id": lockfile.signature_key_id,
        "plugins": lockfile.plugins,
        "packs": lockfile.packs,
        "missing_files": missing,
        "modified_files": modified,
        "unknown_files": unknown,
        "resolved_manifest_sha256": resolved.manifest_sha256,
        "resolved_signature_key_id": resolved.signature_key_id,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize doctor summary: {e}"))?
    );
    Ok(if ok { 0 } else { 1 })
}

pub(crate) async fn run_plugins_cli(parsed: &PluginsCli) -> Result<i32, String> {
    if looks_like_manifest_source(&parsed.registry_source)? {
        let resolved = load_verified_manifest(parsed).await?;
        let json = parse_bool_flag(&parsed.installer_args, "--json");
        return match parsed.action {
            PluginsAction::List => run_plugins_list_manifest(&resolved, json),
            PluginsAction::Packs => run_plugins_packs_manifest(&resolved, json),
            PluginsAction::Info => run_plugins_info_manifest(&resolved, &parsed.installer_args),
            PluginsAction::Install => run_plugins_install_manifest(&resolved, parsed).await,
            PluginsAction::Update => run_plugins_update_manifest(&resolved, parsed).await,
            PluginsAction::Doctor => run_plugins_doctor_manifest(&resolved, parsed),
            PluginsAction::Uninstall => run_plugins_uninstall_manifest(&resolved, parsed),
        };
    }

    let registry_root = cache_registry_source(&parsed.registry_source).await?;
    let installer = registry_root.join("scripts").join("compas_plugins.py");
    if !installer.is_file() {
        return Err(format!(
            "installer is missing in cached registry: {}",
            installer.display()
        ));
    }

    let legacy_args = strip_manifest_only_flags(&parsed.installer_args)?;
    let legacy_parsed = PluginsCli {
        action: parsed.action,
        registry_source: parsed.registry_source.clone(),
        repo_root: parsed.repo_root.clone(),
        installer_args: legacy_args.clone(),
    };

    match parsed.action {
        PluginsAction::Install => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "install",
            &legacy_args,
        ),
        PluginsAction::List => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "list",
            &legacy_args,
        ),
        PluginsAction::Packs => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "packs",
            &legacy_args,
        ),
        PluginsAction::Info => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "info",
            &legacy_args,
        ),
        PluginsAction::Update => run_plugins_update(&installer, &legacy_parsed),
        PluginsAction::Uninstall => run_plugins_uninstall(&installer, &legacy_parsed),
        PluginsAction::Doctor => run_plugins_doctor(&installer, &legacy_parsed),
    }
}
