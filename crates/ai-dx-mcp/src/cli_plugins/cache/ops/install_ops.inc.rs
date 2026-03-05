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
    let allow_sunset_compat = parse_bool_flag(&parsed.installer_args, FLAG_ALLOW_SUNSET_COMPAT);
    let allow_sunset =
        parse_bool_flag(&parsed.installer_args, FLAG_ALLOW_SUNSET) || allow_sunset_compat;
    if allow_sunset_compat {
        eprintln!(
            "compas: compatibility alias `{}` is accepted for now; prefer `{}`.",
            FLAG_ALLOW_SUNSET_COMPAT, FLAG_ALLOW_SUNSET
        );
    }

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
            governance_block_reason(plugin, allow_experimental, allow_sunset)
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
                "allow_sunset": allow_sunset,
                "blocked_plugins": blocked_plugins,
            },
            "hint": "use --allow-experimental and/or --allow-sunset for native registry install/update",
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

