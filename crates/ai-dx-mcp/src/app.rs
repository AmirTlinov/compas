use crate::{
    api::{
        ApiError, BaselineMaintenance, BoundarySummary, DecisionStatus, EffectiveConfigSummary,
        GateKind, GateOutput, InitOutput, InitRequest, LocSummary, PublicSurfaceSummary,
        ValidateMode, ValidateOutput, Violation, ViolationTier,
    },
    checks::{
        arch_layers::run_arch_layers_check,
        boundary::run_boundary_check,
        complexity_budget::run_complexity_budget_check,
        contract_break::run_contract_break_check,
        dead_api::{run_dead_code_check, run_orphan_api_check},
        duplicates::run_duplicates_check,
        env_registry::run_env_registry_check,
        loc::run_loc_check,
        quality_delta::FileUniverse,
        reuse_first::run_reuse_first_check,
        supply_chain::run_supply_chain_check,
        surface::run_surface_check,
        tool_budget::run_tool_budget_check,
    },
    failure_modes::{default_failure_mode_catalog, load_failure_mode_catalog},
    packs::validate_packs,
    repo::{RepoConfigError, load_repo_config},
    validate_insights::{
        build_agent_digest_with_suppressed, build_coverage, build_quality_posture,
        build_risk_summary, build_trust_score, to_findings_v2,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod support;

use support::{
    collect_suppressed_codes, compute_checks_hash, detect_tool_duplicates, empty_output_with_error,
    has_prior_baselines,
};

pub(crate) fn map_config_error(repo_root: &str, err: RepoConfigError) -> ApiError {
    ApiError {
        code: err.code().to_string(),
        message: format!("{err} (repo_root={repo_root})"),
    }
}

pub fn compas_init(repo_root: &str, req: InitRequest) -> InitOutput {
    crate::init::init(repo_root, req)
}

pub fn validate(
    repo_root: &str,
    mode: ValidateMode,
    write_baseline: bool,
    baseline_maintenance: Option<&BaselineMaintenance>,
) -> ValidateOutput {
    let repo_root_path = Path::new(repo_root);
    let cfg = match load_repo_config(repo_root_path) {
        Ok(c) => c,
        Err(e) => {
            return empty_output_with_error(repo_root, mode, map_config_error(repo_root, e), None);
        }
    };

    if write_baseline && matches!(mode, ValidateMode::Ratchet) {
        match baseline_maintenance {
            None => {
                return empty_output_with_error(
                    repo_root,
                    mode,
                    ApiError {
                        code: "config.baseline_write_requires_maintenance".to_string(),
                        message: "write_baseline=true in ratchet mode requires baseline_maintenance with reason (>=20 chars) and owner".to_string(),
                    },
                    None,
                );
            }
            Some(bm) if bm.reason.trim().len() < 20 => {
                return empty_output_with_error(
                    repo_root,
                    mode,
                    ApiError {
                        code: "config.baseline_maintenance_reason_too_short".to_string(),
                        message: format!(
                            "baseline_maintenance.reason must be >=20 chars (got {})",
                            bm.reason.trim().len()
                        ),
                    },
                    None,
                );
            }
            Some(_) => {}
        }
    }

    let mut violations_raw: Vec<Violation> = vec![];
    let mut loc_summary: Option<LocSummary> = None;
    let mut boundary_summary: Option<BoundarySummary> = None;
    let mut public_surface_summary: Option<PublicSurfaceSummary> = None;
    let mut effective_config: Option<EffectiveConfigSummary> = None;
    let mut file_universe = FileUniverse::default();
    let mut loc_per_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut surface_items: BTreeSet<String> = BTreeSet::new();
    let mut duplicate_groups: Vec<Vec<String>> = vec![];

    // P0 anti-gaming: allow_any policy warning is always blocking.
    for plugin_id in &cfg.allow_any_plugins {
        violations_raw.push(Violation::blocking(
            "security.allow_any_policy",
            format!(
                "plugin {plugin_id} uses allow_any tool policy; this bypasses execution safety rails"
            ),
            None,
            None,
        ));
    }

    // Mandatory checks contract.
    if let Some(contract) = &cfg.quality_contract {
        let mut active_check_types: BTreeSet<&str> = BTreeSet::new();
        if !cfg.checks.boundary.is_empty() {
            active_check_types.insert("boundary");
        }
        if !cfg.checks.supply_chain.is_empty() {
            active_check_types.insert("supply_chain");
        }
        if !cfg.checks.loc.is_empty() {
            active_check_types.insert("loc");
        }
        if !cfg.checks.surface.is_empty() {
            active_check_types.insert("surface");
        }
        if !cfg.checks.duplicates.is_empty() {
            active_check_types.insert("duplicates");
        }
        if !cfg.checks.env_registry.is_empty() {
            active_check_types.insert("env_registry");
        }
        if !cfg.checks.tool_budget.is_empty() {
            active_check_types.insert("tool_budget");
        }
        if !cfg.checks.reuse_first.is_empty() {
            active_check_types.insert("reuse_first");
        }
        if !cfg.checks.arch_layers.is_empty() {
            active_check_types.insert("arch_layers");
        }
        if !cfg.checks.dead_code.is_empty() {
            active_check_types.insert("dead_code");
        }
        if !cfg.checks.orphan_api.is_empty() {
            active_check_types.insert("orphan_api");
        }
        if !cfg.checks.complexity_budget.is_empty() {
            active_check_types.insert("complexity_budget");
        }
        if !cfg.checks.contract_break.is_empty() {
            active_check_types.insert("contract_break");
        }
        for mandatory in &contract.governance.mandatory_checks {
            if !active_check_types.contains(mandatory.as_str()) {
                violations_raw.push(Violation::blocking(
                    "config.mandatory_check_removed",
                    format!("mandatory check '{mandatory}' is not configured"),
                    None,
                    None,
                ));
            }
        }
    }

    violations_raw.extend(validate_packs(repo_root_path));
    violations_raw.extend(detect_tool_duplicates(&cfg));

    if !cfg.checks.boundary.is_empty() {
        let mut files_scanned = 0usize;
        let mut rules_checked = 0usize;
        let mut vio_count = 0usize;
        for boundary_cfg in &cfg.checks.boundary {
            match run_boundary_check(repo_root_path, boundary_cfg) {
                Ok(r) => {
                    files_scanned += r.files_scanned;
                    rules_checked += r.rules_checked;
                    vio_count += r.violations.len();
                    violations_raw.extend(r.violations);
                }
                Err(msg) => {
                    violations_raw.push(Violation::blocking(
                        "boundary.check_failed",
                        format!("boundary check failed (id={}): {msg}", boundary_cfg.id),
                        None,
                        None,
                    ));
                }
            }
        }
        file_universe.boundary_universe = files_scanned;
        file_universe.boundary_scanned = files_scanned;
        boundary_summary = Some(BoundarySummary {
            files_scanned,
            rules_checked,
            violations: vio_count,
        });
    }

    if !cfg.checks.loc.is_empty() {
        let mut files_scanned = 0usize;
        let mut max_loc = 0usize;
        let mut files_universe = 0usize;
        let mut worst_path: Option<String> = None;
        for loc_cfg in &cfg.checks.loc {
            match run_loc_check(repo_root_path, loc_cfg) {
                Ok(r) => {
                    files_scanned += r.files_scanned;
                    files_universe += r.files_universe;
                    max_loc = max_loc.max(r.max_loc);
                    if worst_path.is_none() {
                        worst_path = r.worst_path;
                    }
                    for (k, v) in r.loc_per_file {
                        // deterministic max merge for duplicated paths across check instances
                        let entry = loc_per_file.entry(k).or_insert(0);
                        *entry = (*entry).max(v);
                    }
                    violations_raw.extend(r.violations);
                }
                Err(msg) => {
                    violations_raw.push(Violation::blocking(
                        "loc.check_failed",
                        format!("loc check failed (id={}): {msg}", loc_cfg.id),
                        None,
                        None,
                    ));
                }
            }
        }
        file_universe.loc_universe = files_universe;
        file_universe.loc_scanned = files_scanned;
        loc_summary = Some(LocSummary {
            files_scanned,
            max_loc,
            worst_path,
        });
    }

    if !cfg.checks.surface.is_empty() {
        let mut best: Option<(usize, PublicSurfaceSummary)> = None;
        let mut files_scanned = 0usize;
        let mut files_universe = 0usize;
        for surface_cfg in &cfg.checks.surface {
            match run_surface_check(repo_root_path, surface_cfg) {
                Ok(r) => {
                    files_scanned += r.files_scanned;
                    files_universe += r.files_universe;
                    violations_raw.extend(r.violations);
                    surface_items.extend(r.current_items.into_iter());
                    let summary = PublicSurfaceSummary {
                        baseline_path: surface_cfg.baseline_path.clone(),
                        max_pub_items: r.max_items,
                        items_total: r.items_total,
                        added_vs_baseline: 0,
                        removed_vs_baseline: 0,
                    };
                    let score = r.items_total;
                    if best
                        .as_ref()
                        .map(|(best_score, _)| score > *best_score)
                        .unwrap_or(true)
                    {
                        best = Some((score, summary));
                    }
                }
                Err(msg) => {
                    violations_raw.push(Violation::blocking(
                        "surface.check_failed",
                        format!("surface check failed (id={}): {msg}", surface_cfg.id),
                        None,
                        None,
                    ));
                }
            }
        }
        file_universe.surface_universe = files_universe;
        file_universe.surface_scanned = files_scanned;
        public_surface_summary = best.map(|(_, s)| s);
    }

    if !cfg.checks.duplicates.is_empty() {
        let mut files_scanned = 0usize;
        let mut files_universe = 0usize;
        let mut merged_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for dup_cfg in &cfg.checks.duplicates {
            match run_duplicates_check(repo_root_path, dup_cfg) {
                Ok(r) => {
                    files_scanned += r.files_scanned;
                    files_universe += r.files_universe;
                    for (sha, paths) in r.groups {
                        let group = merged_groups.entry(sha).or_default();
                        for p in paths {
                            group.insert(p);
                        }
                    }
                    violations_raw.extend(r.violations);
                }
                Err(msg) => violations_raw.push(Violation::blocking(
                    "duplicates.check_failed",
                    format!("duplicates check failed (id={}): {msg}", dup_cfg.id),
                    None,
                    None,
                )),
            }
        }
        file_universe.duplicates_universe = files_universe;
        file_universe.duplicates_scanned = files_scanned;
        duplicate_groups = merged_groups
            .into_values()
            .map(|set| set.into_iter().collect::<Vec<_>>())
            .collect();
        duplicate_groups.sort();
    }

    if !cfg.checks.supply_chain.is_empty() {
        for sc_cfg in &cfg.checks.supply_chain {
            let out = run_supply_chain_check(repo_root_path, sc_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.tool_budget.is_empty() {
        for budget_cfg in &cfg.checks.tool_budget {
            let out = run_tool_budget_check(&cfg, budget_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.reuse_first.is_empty() {
        for reuse_cfg in &cfg.checks.reuse_first {
            let out = run_reuse_first_check(repo_root_path, reuse_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.arch_layers.is_empty() {
        for layers_cfg in &cfg.checks.arch_layers {
            let out = run_arch_layers_check(repo_root_path, layers_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.dead_code.is_empty() {
        for dead_cfg in &cfg.checks.dead_code {
            let out = run_dead_code_check(repo_root_path, dead_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.orphan_api.is_empty() {
        for orphan_cfg in &cfg.checks.orphan_api {
            let out = run_orphan_api_check(repo_root_path, orphan_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.complexity_budget.is_empty() {
        for cx_cfg in &cfg.checks.complexity_budget {
            let out = run_complexity_budget_check(repo_root_path, cx_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if !cfg.checks.contract_break.is_empty() {
        for contract_cfg in &cfg.checks.contract_break {
            let out = run_contract_break_check(repo_root_path, contract_cfg);
            violations_raw.extend(out.violations);
        }
    }

    if let Some(env_cfg) = cfg.checks.env_registry.first() {
        let env_result = run_env_registry_check(repo_root_path, env_cfg, &cfg.tools);
        violations_raw.extend(env_result.violations);
        effective_config = Some(env_result.summary);
    }

    // quality_contract mode-aware presence signal
    if cfg.quality_contract.is_none() {
        let tier = match mode {
            ValidateMode::Ratchet | ValidateMode::Strict => ViolationTier::Blocking,
            ValidateMode::Warn => ViolationTier::Observation,
        };
        violations_raw.push(Violation {
            code: "config.quality_contract_missing".to_string(),
            message: "quality_contract.toml not found under .agents/mcp/compas/".to_string(),
            path: Some(".agents/mcp/compas/quality_contract.toml".to_string()),
            details: None,
            tier,
        });
    }

    let failure_mode_catalog = match load_failure_mode_catalog(repo_root_path) {
        Ok(catalog) => catalog,
        Err(e) => {
            violations_raw.push(Violation::blocking(
                "failure_modes.invalid",
                e.to_string(),
                Some(e.path.display().to_string()),
                None,
            ));
            default_failure_mode_catalog()
        }
    };

    let max_exception_window_days = cfg
        .quality_contract
        .as_ref()
        .map(|c| c.exceptions.max_exception_window_days);
    let suppression = if let Some(max_days) = max_exception_window_days {
        crate::exceptions::apply_allowlist_with_limits(
            repo_root_path,
            violations_raw.clone(),
            Some(max_days),
        )
    } else {
        crate::exceptions::apply_allowlist(repo_root_path, violations_raw.clone())
    };

    // Phase 1 insights split: raw vs display(post-suppress)
    let findings_raw = to_findings_v2(&violations_raw);
    let risk_raw = build_risk_summary(&findings_raw);
    let coverage_raw = build_coverage(&failure_mode_catalog, repo_root_path, &cfg);
    let quality_posture = build_quality_posture(&findings_raw, &coverage_raw, &risk_raw);

    // Additional non-suppressible phase2/policy violations
    let mut phase2_violations: Vec<Violation> = vec![];

    if let Some(contract) = &cfg.quality_contract {
        // Mandatory failure-modes catalog guards
        for mandatory in &contract.governance.mandatory_failure_modes {
            if !failure_mode_catalog.contains(mandatory) {
                phase2_violations.push(Violation::blocking(
                    "failure_modes.mandatory_missing",
                    format!("mandatory failure mode '{mandatory}' not in catalog"),
                    Some(".agents/mcp/compas/failure_modes.toml".to_string()),
                    None,
                ));
            }
        }
        if failure_mode_catalog.len() < contract.governance.min_failure_modes {
            phase2_violations.push(Violation::blocking(
                "failure_modes.catalog_too_small",
                format!(
                    "failure mode catalog has {} modes, minimum is {}",
                    failure_mode_catalog.len(),
                    contract.governance.min_failure_modes
                ),
                Some(".agents/mcp/compas/failure_modes.toml".to_string()),
                None,
            ));
        }

        // Exception budget
        let suppressed_count = suppression.suppressed.len();
        if suppressed_count > contract.exceptions.max_exceptions {
            phase2_violations.push(Violation::blocking(
                "exception.budget_exceeded",
                format!(
                    "suppressed violations ({suppressed_count}) exceed max_exceptions ({})",
                    contract.exceptions.max_exceptions
                ),
                None,
                None,
            ));
        }
        let total_before_suppress = violations_raw.len();
        if total_before_suppress > 0 {
            let ratio = suppressed_count as f64 / total_before_suppress as f64;
            if ratio > contract.exceptions.max_suppressed_ratio {
                phase2_violations.push(Violation::blocking(
                    "exception.budget_exceeded",
                    format!(
                        "suppressed ratio {:.2} exceeds max_suppressed_ratio {:.2}",
                        ratio, contract.exceptions.max_suppressed_ratio
                    ),
                    None,
                    Some(serde_json::json!({
                        "suppressed_count": suppressed_count,
                        "total_before_suppress": total_before_suppress,
                        "ratio": ratio
                    })),
                ));
            }
        }

        let config_hash = compute_checks_hash(&cfg);
        if let Some(locked_hash) = &contract.governance.config_hash
            && locked_hash != &config_hash
        {
            phase2_violations.push(Violation::blocking(
                "config.threshold_weakened",
                format!("config hash differs from locked governance hash: expected={locked_hash}, current={config_hash}"),
                Some(".agents/mcp/compas/quality_contract.toml".to_string()),
                None,
            ));
        }

        let snapshot_path = repo_root_path.join(&contract.baseline.snapshot_path);

        if matches!(mode, ValidateMode::Ratchet)
            && !write_baseline
            && !snapshot_path.is_file()
            && has_prior_baselines(repo_root_path)
        {
            match crate::checks::quality_delta::migrate_from_prior_baselines(
                repo_root_path,
                quality_posture.trust_score,
                quality_posture.coverage_covered,
                quality_posture.coverage_total,
                quality_posture.weighted_risk,
                &config_hash,
            ) {
                Ok(s) => {
                    if let Err(e) = crate::checks::quality_delta::write_snapshot(&snapshot_path, &s)
                    {
                        phase2_violations.push(Violation::blocking(
                            "quality_delta.check_failed",
                            format!("prior baseline migration write failed: {e}"),
                            Some(snapshot_path.display().to_string()),
                            None,
                        ));
                    }
                }
                Err(e) => phase2_violations.push(Violation::blocking(
                    "quality_delta.check_failed",
                    format!("prior baseline migration failed: {e}"),
                    Some(snapshot_path.display().to_string()),
                    None,
                )),
            }
        }

        let mut surface_items_sorted = surface_items.into_iter().collect::<Vec<_>>();
        surface_items_sorted.sort();
        let current_snapshot = crate::checks::quality_delta::QualitySnapshot {
            version: 1,
            trust_score: quality_posture.trust_score,
            coverage_covered: quality_posture.coverage_covered,
            coverage_total: quality_posture.coverage_total,
            weighted_risk: quality_posture.weighted_risk,
            findings_total: quality_posture.findings_total,
            risk_by_severity: quality_posture.risk_by_severity.clone(),
            loc_per_file,
            surface_items: surface_items_sorted,
            duplicate_groups,
            file_universe,
            written_at: chrono::Utc::now().to_rfc3339(),
            written_by: baseline_maintenance.cloned(),
            config_hash,
        };

        match crate::checks::quality_delta::run_quality_delta(
            &snapshot_path,
            contract,
            &current_snapshot,
            matches!(mode, ValidateMode::Ratchet),
            write_baseline,
            baseline_maintenance,
        ) {
            Ok(delta) => {
                phase2_violations.extend(delta.violations);
            }
            Err(e) => {
                phase2_violations.push(Violation::blocking(
                    "quality_delta.check_failed",
                    e,
                    Some(snapshot_path.display().to_string()),
                    None,
                ));
            }
        }
    }

    let mut final_violations = suppression.violations;
    final_violations.extend(phase2_violations);
    let findings_display = to_findings_v2(&final_violations);
    let risk_display = build_risk_summary(&findings_display);
    let coverage_display = build_coverage(&failure_mode_catalog, repo_root_path, &cfg);
    let trust_display = build_trust_score(
        &findings_display,
        final_violations.is_empty() || matches!(mode, ValidateMode::Warn),
        coverage_display.percent,
    );
    let suppressed = suppression.suppressed;
    let mut verdict = crate::judge::judge_validate(&final_violations, mode);
    verdict.quality_posture = Some(quality_posture.clone());
    verdict.suppressed_count = suppressed.len();
    verdict.suppressed_codes = collect_suppressed_codes(&suppressed);
    let agent_digest = build_agent_digest_with_suppressed(
        &verdict.decision,
        &final_violations,
        &findings_display,
        &suppressed,
    );
    let ok = matches!(mode, ValidateMode::Warn)
        || matches!(verdict.decision.status, DecisionStatus::Pass);

    ValidateOutput {
        ok,
        error: None,
        schema_version: "3".to_string(),
        repo_root: repo_root.to_string(),
        mode,
        violations: final_violations,
        findings_v2: findings_display,
        suppressed,
        loc: loc_summary,
        boundary: boundary_summary,
        public_surface: public_surface_summary,
        effective_config,
        risk_summary: Some(risk_display),
        coverage: Some(coverage_display),
        trust_score: Some(trust_display),
        verdict: Some(verdict),
        quality_posture: Some(quality_posture),
        agent_digest: Some(agent_digest),
        summary_md: None,
        payload_meta: None,
    }
}

pub async fn gate(
    repo_root: &str,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
) -> GateOutput {
    gate_with_budget(repo_root, kind, dry_run, write_witness, None).await
}

pub async fn gate_with_budget(
    repo_root: &str,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
    gate_budget_ms: Option<u64>,
) -> GateOutput {
    let mut out =
        crate::gate_runner::gate(repo_root, kind, dry_run, write_witness, gate_budget_ms).await;
    let suppressed_codes = collect_suppressed_codes(&out.validate.suppressed);
    let suppressed_count = out.validate.suppressed.len();

    if let Some(verdict) = out.verdict.as_mut() {
        verdict.quality_posture = out.validate.quality_posture.clone();
        verdict.suppressed_count = suppressed_count;
        verdict.suppressed_codes = suppressed_codes;
    }

    if let Some(verdict) = out.verdict.as_ref() {
        out.agent_digest = Some(build_agent_digest_with_suppressed(
            &verdict.decision,
            &out.validate.violations,
            &out.validate.findings_v2,
            &out.validate.suppressed,
        ));
    }

    out
}
