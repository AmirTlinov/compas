use crate::api::{BaselineMaintenance, Violation, ViolationTier};
use crate::config::QualityContractConfig;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySnapshot {
    pub version: u32,
    // Raw holistic posture (pre-suppress)
    pub trust_score: i32,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: BTreeMap<String, usize>,
    // Granular ratchets
    pub loc_per_file: BTreeMap<String, usize>,
    pub surface_items: Vec<String>,
    pub duplicate_groups: Vec<Vec<String>>,
    // Scope tracking
    pub file_universe: FileUniverse,
    // Provenance
    pub written_at: String,
    pub written_by: Option<BaselineMaintenance>,
    pub config_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileUniverse {
    pub loc_universe: usize,
    pub loc_scanned: usize,
    pub surface_universe: usize,
    pub surface_scanned: usize,
    pub boundary_universe: usize,
    pub boundary_scanned: usize,
    pub duplicates_universe: usize,
    pub duplicates_scanned: usize,
}

#[derive(Debug, Clone)]
pub struct QualityDeltaResult {
    pub violations: Vec<Violation>,
    pub baseline_loaded: bool,
}

pub fn load_snapshot_from_str(json: &str) -> Result<QualitySnapshot, String> {
    let snap: QualitySnapshot =
        serde_json::from_str(json).map_err(|e| format!("failed to parse quality snapshot: {e}"))?;
    if snap.version > SNAPSHOT_VERSION {
        return Err(format!(
            "quality snapshot version {} > supported max {}",
            snap.version, SNAPSHOT_VERSION
        ));
    }
    Ok(snap)
}

pub fn load_snapshot(path: &Path) -> Result<Option<QualitySnapshot>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read quality snapshot {:?}: {e}", path))?;
    load_snapshot_from_str(&raw).map(Some)
}

pub fn write_snapshot(path: &Path, snapshot: &QualitySnapshot) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create snapshot dir {:?}: {e}", parent))?;
    }
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("failed to serialize snapshot: {e}"))?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, &json)
        .map_err(|e| format!("failed to write snapshot tmp {:?}: {e}", tmp))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("failed to rename snapshot {:?}: {e}", path)
    })?;
    Ok(())
}

fn blocking(code: &str, message: String, details: Option<serde_json::Value>) -> Violation {
    Violation {
        code: code.to_string(),
        message,
        path: None,
        details,
        tier: ViolationTier::Blocking,
    }
}

fn check_scope_narrowing(
    baseline: &FileUniverse,
    current: &FileUniverse,
    max_narrowing: f64,
    out: &mut Vec<Violation>,
) {
    let domains = [
        (
            "loc",
            baseline.loc_scanned,
            baseline.loc_universe,
            current.loc_scanned,
            current.loc_universe,
        ),
        (
            "surface",
            baseline.surface_scanned,
            baseline.surface_universe,
            current.surface_scanned,
            current.surface_universe,
        ),
        (
            "boundary",
            baseline.boundary_scanned,
            baseline.boundary_universe,
            current.boundary_scanned,
            current.boundary_universe,
        ),
        (
            "duplicates",
            baseline.duplicates_scanned,
            baseline.duplicates_universe,
            current.duplicates_scanned,
            current.duplicates_universe,
        ),
    ];
    for (domain, b_scanned, b_universe, c_scanned, c_universe) in domains {
        if b_universe == 0 || c_universe == 0 {
            continue;
        }
        let base_ratio = b_scanned as f64 / b_universe as f64;
        let curr_ratio = c_scanned as f64 / c_universe as f64;
        let drop = base_ratio - curr_ratio;
        if drop > max_narrowing {
            out.push(blocking(
                "quality_delta.scope_narrowed",
                format!(
                    "scan ratio dropped for {domain}: baseline={base_ratio:.2}, current={curr_ratio:.2}, drop={drop:.2}, max={max_narrowing:.2}"
                ),
                Some(json!({
                    "domain": domain,
                    "baseline_ratio": base_ratio,
                    "current_ratio": curr_ratio,
                    "drop": drop,
                    "max_narrowing": max_narrowing
                })),
            ));
        }
    }
}

pub fn compare(
    baseline: &QualitySnapshot,
    current: &QualitySnapshot,
    contract: &QualityContractConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    if !contract.quality.allow_trust_drop && current.trust_score < baseline.trust_score {
        violations.push(blocking(
            "quality_delta.trust_regression",
            format!(
                "trust score regressed: baseline={}, current={}",
                baseline.trust_score, current.trust_score
            ),
            None,
        ));
    }

    if current.trust_score < contract.quality.min_trust_score {
        violations.push(blocking(
            "quality_delta.trust_below_minimum",
            format!(
                "trust score {} below minimum {}",
                current.trust_score, contract.quality.min_trust_score
            ),
            None,
        ));
    }

    if !contract.quality.allow_coverage_drop && current.coverage_covered < baseline.coverage_covered
    {
        violations.push(blocking(
            "quality_delta.coverage_regression",
            format!(
                "coverage regressed: baseline={}, current={}",
                baseline.coverage_covered, current.coverage_covered
            ),
            None,
        ));
    }

    if current.coverage_total > 0 {
        let percent = (current.coverage_covered as f64 / current.coverage_total as f64) * 100.0;
        if percent < contract.quality.min_coverage_percent {
            violations.push(blocking(
                "quality_delta.coverage_below_minimum",
                format!(
                    "coverage {:.2}% below minimum {:.2}%",
                    percent, contract.quality.min_coverage_percent
                ),
                Some(json!({
                    "coverage_percent": percent,
                    "min_coverage_percent": contract.quality.min_coverage_percent
                })),
            ));
        }
    }

    let risk_increase = current.weighted_risk - baseline.weighted_risk;
    if risk_increase > contract.quality.max_weighted_risk_increase {
        violations.push(blocking(
            "quality_delta.risk_profile_regression",
            format!(
                "weighted risk increased: baseline={}, current={}, increase={}, max_allowed={}",
                baseline.weighted_risk,
                current.weighted_risk,
                risk_increase,
                contract.quality.max_weighted_risk_increase
            ),
            None,
        ));
    }

    for (path, current_loc) in &current.loc_per_file {
        if let Some(base_loc) = baseline.loc_per_file.get(path)
            && current_loc > base_loc
        {
            violations.push(Violation {
                code: "quality_delta.loc_regression".to_string(),
                message: format!("LOC grew: {path} baseline={base_loc} current={current_loc}"),
                path: Some(path.clone()),
                details: None,
                tier: ViolationTier::Blocking,
            });
        }
    }

    let baseline_set: BTreeSet<&String> = baseline.surface_items.iter().collect();
    let added: Vec<&String> = current
        .surface_items
        .iter()
        .filter(|item| !baseline_set.contains(item))
        .collect();
    if !added.is_empty() {
        violations.push(blocking(
            "quality_delta.surface_regression",
            format!("new public surface items: {} added", added.len()),
            Some(json!({"added_count": added.len(), "added_examples": added.iter().take(10).cloned().collect::<Vec<_>>() })),
        ));
    }

    let baseline_dup: BTreeSet<Vec<String>> = baseline.duplicate_groups.iter().cloned().collect();
    let new_groups: Vec<&Vec<String>> = current
        .duplicate_groups
        .iter()
        .filter(|g| !baseline_dup.contains(*g))
        .collect();
    if !new_groups.is_empty() {
        violations.push(blocking(
            "quality_delta.duplicates_regression",
            format!("new duplicate groups: {} added", new_groups.len()),
            Some(json!({"new_groups": new_groups.len() })),
        ));
    }

    check_scope_narrowing(
        &baseline.file_universe,
        &current.file_universe,
        contract.baseline.max_scope_narrowing,
        &mut violations,
    );

    if baseline.config_hash != current.config_hash {
        violations.push(blocking(
            "quality_delta.config_changed",
            format!(
                "config hash changed: baseline={}, current={}",
                baseline.config_hash, current.config_hash
            ),
            None,
        ));
    }

    violations
}

pub fn run_quality_delta(
    snapshot_path: &Path,
    contract: &QualityContractConfig,
    current: &QualitySnapshot,
    mode_ratchet: bool,
    write_baseline: bool,
    maintenance: Option<&BaselineMaintenance>,
) -> Result<QualityDeltaResult, String> {
    let baseline = load_snapshot(snapshot_path)?;
    let mut violations = Vec::new();

    if mode_ratchet && !write_baseline {
        if let Some(base) = &baseline {
            violations.extend(compare(base, current, contract));
        } else {
            if current.trust_score < contract.quality.min_trust_score {
                violations.push(blocking(
                    "quality_delta.trust_below_minimum",
                    format!(
                        "trust score {} below minimum {}",
                        current.trust_score, contract.quality.min_trust_score
                    ),
                    None,
                ));
            }
            if current.coverage_total > 0 {
                let percent =
                    (current.coverage_covered as f64 / current.coverage_total as f64) * 100.0;
                if percent < contract.quality.min_coverage_percent {
                    violations.push(blocking(
                        "quality_delta.coverage_below_minimum",
                        format!(
                            "coverage {:.2}% below minimum {:.2}%",
                            percent, contract.quality.min_coverage_percent
                        ),
                        Some(json!({
                            "coverage_percent": percent,
                            "min_coverage_percent": contract.quality.min_coverage_percent
                        })),
                    ));
                }
            }
        }
    }

    if write_baseline {
        if mode_ratchet {
            let maint = maintenance.ok_or_else(|| {
                "write_baseline=true in ratchet mode requires baseline_maintenance".to_string()
            })?;
            if maint.reason.trim().len() < 20 {
                return Err(format!(
                    "baseline_maintenance.reason must be >=20 chars (got {})",
                    maint.reason.trim().len()
                ));
            }
        }
        write_snapshot(snapshot_path, current)?;
    }

    Ok(QualityDeltaResult {
        violations,
        baseline_loaded: baseline.is_some(),
    })
}

pub fn migrate_from_prior_baselines(
    repo_root: &Path,
    trust_score: i32,
    coverage_covered: usize,
    coverage_total: usize,
    weighted_risk: i32,
    config_hash: &str,
) -> Result<QualitySnapshot, String> {
    let baselines_dir = repo_root.join(".agents/mcp/compas/baselines");

    let loc_per_file = {
        let path = baselines_dir.join("loc.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::loc::LocBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline.files
        } else {
            BTreeMap::new()
        }
    };

    let surface_items = {
        let path = baselines_dir.join("public_surface.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::surface::SurfaceBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline.items
        } else {
            vec![]
        }
    };

    let duplicate_groups = {
        let path = baselines_dir.join("duplicates.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::duplicates::DuplicatesBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline
                .groups
                .into_iter()
                .map(|g| {
                    let mut paths = g.paths;
                    paths.sort();
                    paths
                })
                .collect()
        } else {
            vec![]
        }
    };

    Ok(QualitySnapshot {
        version: SNAPSHOT_VERSION,
        trust_score,
        coverage_covered,
        coverage_total,
        weighted_risk,
        findings_total: 0,
        risk_by_severity: BTreeMap::new(),
        loc_per_file,
        surface_items,
        duplicate_groups,
        file_universe: FileUniverse::default(),
        written_at: Utc::now().to_rfc3339(),
        written_by: None,
        config_hash: config_hash.to_string(),
    })
}

#[cfg(test)]
mod tests;
