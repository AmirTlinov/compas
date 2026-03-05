struct ManifestResolved {
    manifest: RegistryManifestV1,
    manifest_sha256: String,
    signature_key_id: Option<String>,
    base_url: Option<String>,
    base_dir: Option<PathBuf>,
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

fn plugin_sunset_marker(plugin: &RegistryPluginV1) -> Option<&serde_json::Value> {
    plugin.extra.get(SUNSET_META_COMPAT_KEY)
}

