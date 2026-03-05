use super::*;

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
            "sunset": plugin_sunset_marker(plugin),
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
        "sunset": plugin_sunset_marker(plugin),
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
    allow_sunset: bool,
) -> Option<(String, String)> {
    let sunset_meta_present = plugin
        .extra
        .get(SUNSET_META_COMPAT_KEY)
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
        TIER_EXPERIMENTAL if !allow_experimental => Some((
            tier,
            "install is blocked: tier=experimental (add --allow-experimental to proceed)"
                .to_string(),
        )),
        t if (t == TIER_SUNSET || t == SUNSET_META_COMPAT_KEY) && !allow_sunset => Some((
            tier,
            "install is blocked: tier=sunset (add --allow-sunset to proceed)".to_string(),
        )),
        _ if sunset_meta_present && !allow_sunset => Some((
            TIER_SUNSET.to_string(),
            "install is blocked: sunset marker metadata present (add --allow-sunset to proceed)"
                .to_string(),
        )),
        _ => None,
    }
}

include!("ops/install_ops.inc.rs");

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
    ensure_admin_lane(parsed.action, &parsed.installer_args)?;
    let resolved = load_verified_manifest(parsed).await?;
    let json = parse_bool_flag(&parsed.installer_args, "--json");
    match parsed.action {
        PluginsAction::List => run_plugins_list_manifest(&resolved, json),
        PluginsAction::Packs => run_plugins_packs_manifest(&resolved, json),
        PluginsAction::Info => run_plugins_info_manifest(&resolved, &parsed.installer_args),
        PluginsAction::Install => run_plugins_install_manifest(&resolved, parsed).await,
        PluginsAction::Update => run_plugins_update_manifest(&resolved, parsed).await,
        PluginsAction::Doctor => run_plugins_doctor_manifest(&resolved, parsed),
        PluginsAction::Uninstall => run_plugins_uninstall_manifest(&resolved, parsed),
    }
}
