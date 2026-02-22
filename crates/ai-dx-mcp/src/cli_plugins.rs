use super::{PluginsAction, PluginsCli};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

const REGISTRY_STATE_REL_PATH: &str = ".agents/mcp/compas/plugins/.registry_state.json";

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

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in WalkDir::new(src) {
        let entry = entry.map_err(|e| format!("failed to walk {}: {e}", src.display()))?;
        let path = entry.path();
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
    fs::create_dir_all(target_dir)
        .map_err(|e| format!("failed to create extract dir {}: {e}", target_dir.display()))?;
    let mut cmd = Command::new("tar");
    cmd.arg(if is_gzip_archive(archive_path)? {
        "-xzf"
    } else {
        "-xf"
    });
    let status = cmd.arg(archive_path).arg("-C").arg(target_dir).status();
    let status =
        status.map_err(|e| format!("failed to run tar for {}: {e}", archive_path.display()))?;
    if !status.success() {
        return Err(format!(
            "failed to extract archive {} (tar exit: {:?})",
            archive_path.display(),
            status.code()
        ));
    }
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

pub(crate) async fn run_plugins_cli(parsed: &PluginsCli) -> Result<i32, String> {
    let registry_root = cache_registry_source(&parsed.registry_source).await?;
    let installer = registry_root.join("scripts").join("compas_plugins.py");
    if !installer.is_file() {
        return Err(format!(
            "installer is missing in cached registry: {}",
            installer.display()
        ));
    }

    match parsed.action {
        PluginsAction::Install => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "install",
            &parsed.installer_args,
        ),
        PluginsAction::List => run_installer_status(
            &installer,
            Path::new(&parsed.repo_root),
            "list",
            &parsed.installer_args,
        ),
        PluginsAction::Update => run_plugins_update(&installer, parsed),
        PluginsAction::Uninstall => run_plugins_uninstall(&installer, parsed),
        PluginsAction::Doctor => run_plugins_doctor(&installer, parsed),
    }
}
