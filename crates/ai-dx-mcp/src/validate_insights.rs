use crate::{
    api::{
        AgentDigest, CoverageSummary, Decision, FindingDetailsV2, FindingSeverity, FindingV2,
        QualityPosture, RiskSummary, TrustScore, TrustWeights, Violation, ViolationTier,
    },
    repo::RepoConfig,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn boundary_rule_id(v: &Violation) -> Option<&str> {
    v.details
        .as_ref()
        .and_then(|d| d.get("rule_id"))
        .and_then(|v| v.as_str())
}

fn boundary_rule_category(rule_id: &str) -> Option<&'static str> {
    if rule_id == "no-runtime-unwrap-expect" || rule_id == "no-runtime-panic" {
        Some("resilience_defaults")
    } else if rule_id == "no-runtime-stdout" {
        Some("fail_open")
    } else {
        None
    }
}

fn finding_category(v: &Violation) -> &'static str {
    let code = v.code.as_str();
    if code == "boundary.rule_violation"
        && let Some(rule_id) = boundary_rule_id(v)
        && let Some(category) = boundary_rule_category(rule_id)
    {
        return category;
    }

    if code.starts_with("boundary.") || code.starts_with("exception.") {
        "policy_theater"
    } else if code.starts_with("loc.") {
        "god_module_cycles"
    } else if code.starts_with("surface.") {
        "public_surface_bloat"
    } else if code.starts_with("env_registry.") {
        "env_sprawl"
    } else if code.starts_with("duplicates.") || code.starts_with("reuse_first.") {
        "unplugged_iron"
    } else if code.starts_with("arch_layers.") {
        "policy_theater"
    } else if code.starts_with("dead_code.") {
        "unplugged_iron"
    } else if code.starts_with("orphan_api.") {
        "public_surface_bloat"
    } else if code.starts_with("complexity_budget.") {
        "god_module_cycles"
    } else if code.starts_with("contract_break.") || code.starts_with("change_impact.") {
        "policy_theater"
    } else if code.starts_with("supply_chain.") {
        "dependency_hygiene"
    } else if code.starts_with("tool_budget.")
        || code.starts_with("quality_delta.")
        || code.starts_with("gate.")
        || code.starts_with("witness.")
    {
        "policy_theater"
    } else if code.starts_with("tools.duplicate_") {
        "unplugged_iron"
    } else {
        "general"
    }
}

fn finding_severity(code: &str) -> FindingSeverity {
    if code.contains("read_failed") || code.contains("check_failed") {
        FindingSeverity::High
    } else if code.starts_with("quality_delta.")
        || code.starts_with("security.allow_any_policy")
        || code.starts_with("config.threshold_weakened")
        || code.starts_with("config.mandatory_check_removed")
        || code.starts_with("contract_break.removed_symbol")
    {
        FindingSeverity::Critical
    } else if code.starts_with("boundary.")
        || code.starts_with("supply_chain.")
        || code.starts_with("env_registry.")
        || code.starts_with("exception.allowlist_invalid")
        || code.starts_with("arch_layers.")
        || code.starts_with("change_impact.")
        || code.starts_with("reuse_first.")
    {
        FindingSeverity::High
    } else if code.starts_with("surface.")
        || code.starts_with("loc.")
        || code.starts_with("tool_budget.")
        || code.starts_with("dead_code.")
        || code.starts_with("orphan_api.")
        || code.starts_with("complexity_budget.")
    {
        FindingSeverity::Medium
    } else {
        FindingSeverity::Low
    }
}

fn finding_fix_recipe(v: &Violation) -> Option<&'static str> {
    let code = v.code.as_str();
    if code == "boundary.rule_violation"
        && let Some(rule_id) = boundary_rule_id(v)
    {
        match rule_id {
            "no-runtime-unwrap-expect" => {
                return Some(
                    "Replace unwrap/expect with explicit error handling and stable error codes in runtime path.",
                );
            }
            "no-runtime-panic" => {
                return Some(
                    "Remove panic! from runtime path and convert to explicit error propagation with diagnostics.",
                );
            }
            "no-runtime-stdout" => {
                return Some(
                    "Use structured diagnostics instead of println!/eprintln! in runtime path.",
                );
            }
            _ => {}
        }
    }

    if code.starts_with("boundary.") {
        Some(
            "Tighten module boundaries: remove forbidden pattern and keep adapter->core dependency direction.",
        )
    } else if code.starts_with("loc.") {
        Some(
            "Split the large file/module into focused slices; keep behavior unchanged while reducing LOC.",
        )
    } else if code.starts_with("surface.") {
        Some(
            "Reduce public API surface or update baseline intentionally with a documented compatibility note.",
        )
    } else if code.starts_with("env_registry.") {
        Some(
            "Register env var in env_registry.toml with description/default/sensitivity and wire used_by_tools.",
        )
    } else if code.starts_with("duplicates.") {
        Some("Extract shared logic into one helper/module and remove duplicated implementations.")
    } else if code.starts_with("reuse_first.") {
        Some(
            "Reuse existing equivalent implementation; remove duplicate code path and reference the canonical utility.",
        )
    } else if code.starts_with("arch_layers.") {
        Some(
            "Restore allowed dependency direction between layers and remove forbidden cross-layer imports.",
        )
    } else if code.starts_with("dead_code.") {
        Some("Remove unused private code or wire it into the real runtime path with tests.")
    } else if code.starts_with("orphan_api.") {
        Some("Remove/privatize unused public export or add real consumers and compatibility tests.")
    } else if code.starts_with("complexity_budget.") {
        Some(
            "Split complex function into focused units until function length and complexity budgets are green.",
        )
    } else if code.starts_with("contract_break.") {
        Some(
            "Align API/schema changes with compatibility policy or update contract baseline through approved change process.",
        )
    } else if code.starts_with("change_impact.") {
        Some(
            "Update impact mapping so changed paths require the correct gate tools and rerun gate.",
        )
    } else if code.starts_with("supply_chain.lockfile_missing") {
        Some(
            "Add and commit the ecosystem lockfile (Cargo.lock / package-lock.json / pnpm-lock.yaml / poetry.lock) before merge.",
        )
    } else if code.starts_with("supply_chain.prerelease_dependency") {
        Some(
            "Replace prerelease dependency with a stable release or explicitly isolate it behind an experimental lane.",
        )
    } else if code.starts_with("supply_chain.") {
        Some("Fix manifest/lockfile hygiene and rerun validate/gate.")
    } else if code.starts_with("tool_budget.") {
        Some(
            "Reduce tool/check/gate fan-out or raise budget intentionally with an explicit DX rationale.",
        )
    } else if code.starts_with("quality_delta.") {
        Some(
            "Restore quality posture to baseline (trust/coverage/risk/loc/surface/duplicates) or refresh baseline via approved maintenance window.",
        )
    } else if code.starts_with("tools.duplicate_exact") {
        Some("Remove exact duplicate tool definitions or consolidate to one canonical tool entry.")
    } else if code.starts_with("tools.duplicate_semantic") {
        Some("Review semantically similar tools and merge if they duplicate developer intent.")
    } else if code.starts_with("exception.") {
        Some(
            "Fix allowlist entry or expiry and rerun validate/gate to keep suppressions explicit and bounded.",
        )
    } else {
        None
    }
}

fn to_finding_v2(v: &Violation) -> FindingV2 {
    FindingV2 {
        code: format!("finding.{}", v.code),
        message: v.message.clone(),
        path: v.path.clone(),
        details: FindingDetailsV2 {
            severity: finding_severity(&v.code),
            category: finding_category(v).to_string(),
            confidence: "high".to_string(),
            evidence_refs: vec![],
            fix_recipe: finding_fix_recipe(v).map(ToString::to_string),
            legacy_details: v.details.clone(),
        },
    }
}

pub(crate) fn to_findings_v2(violations: &[Violation]) -> Vec<FindingV2> {
    let mut findings_v2: Vec<FindingV2> = violations.iter().map(to_finding_v2).collect();
    findings_v2.sort_by(|a, b| a.code.cmp(&b.code).then_with(|| a.path.cmp(&b.path)));
    findings_v2
}

pub(crate) fn build_risk_summary(findings_v2: &[FindingV2]) -> RiskSummary {
    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_severity: BTreeMap<String, usize> = BTreeMap::new();
    for f in findings_v2 {
        *by_category.entry(f.details.category.clone()).or_insert(0) += 1;
        *by_severity
            .entry(format!("{:?}", f.details.severity).to_lowercase())
            .or_insert(0) += 1;
    }
    RiskSummary {
        findings_total: findings_v2.len(),
        by_category,
        by_severity,
    }
}

pub(crate) fn build_coverage(
    catalog: &[String],
    repo_root: &Path,
    cfg: &RepoConfig,
) -> CoverageSummary {
    if catalog.is_empty() {
        return CoverageSummary {
            catalog_total: 0,
            catalog_covered: 0,
            percent: 0.0,
            covered_modes: vec![],
            uncovered_modes: vec![],
            effective_covered_modes: vec![],
            declared_but_ineffective_modes: vec![],
        };
    }
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut ineffective: BTreeSet<String> = BTreeSet::new();
    let has_boundary_rule = |id: &str| {
        cfg.checks
            .boundary
            .iter()
            .flat_map(|b| b.rules.iter())
            .any(|r| r.id == id)
    };
    let has_effective_boundary = cfg.checks.boundary.iter().any(|b| !b.rules.is_empty());
    let has_effective_surface = cfg.checks.surface.iter().any(|s| !s.rules.is_empty());
    let has_effective_loc = cfg.checks.loc.iter().any(|l| l.max_loc < 10_000);

    if has_effective_boundary {
        covered.insert("policy_theater".to_string());
    }
    if !cfg.checks.boundary.is_empty() && !has_effective_boundary {
        ineffective.insert("policy_theater".to_string());
    }
    if !cfg.checks.tool_budget.is_empty() {
        covered.insert("policy_theater".to_string());
    }
    if has_boundary_rule("no-runtime-stdout") {
        covered.insert("fail_open".to_string());
    }
    if !cfg.checks.duplicates.is_empty() {
        covered.insert("unplugged_iron".to_string());
    }
    if !cfg.checks.reuse_first.is_empty() || !cfg.checks.dead_code.is_empty() {
        covered.insert("unplugged_iron".to_string());
    }
    if !cfg.checks.env_registry.is_empty() {
        covered.insert("env_sprawl".to_string());
    }
    if has_effective_surface {
        covered.insert("public_surface_bloat".to_string());
    }
    if !cfg.checks.orphan_api.is_empty() {
        covered.insert("public_surface_bloat".to_string());
    }
    if has_effective_loc {
        covered.insert("god_module_cycles".to_string());
    }
    if !cfg.checks.complexity_budget.is_empty() {
        covered.insert("god_module_cycles".to_string());
    }
    if has_boundary_rule("no-runtime-unwrap-expect")
        || has_boundary_rule("no-runtime-panic")
        || has_effective_loc
    {
        covered.insert("resilience_defaults".to_string());
    }
    if !cfg.checks.arch_layers.is_empty() || !cfg.checks.contract_break.is_empty() {
        covered.insert("policy_theater".to_string());
    }
    if repo_root.join(".agents/skills").is_dir() {
        covered.insert("knowledge_continuity".to_string());
    }
    if !cfg.checks.supply_chain.is_empty() {
        covered.insert("security_baseline".to_string());
        covered.insert("dependency_hygiene".to_string());
    }
    if !cfg.gate.flagship.is_empty() && cfg.checks.supply_chain.is_empty() {
        ineffective.insert("security_baseline".to_string());
        ineffective.insert("dependency_hygiene".to_string());
    }

    let uncovered: Vec<String> = catalog
        .iter()
        .filter(|c| !covered.contains(c.as_str()))
        .map(|c| c.to_string())
        .collect();
    let percent = ((covered.len() as f64 / catalog.len() as f64) * 100.0 * 100.0).round() / 100.0;

    CoverageSummary {
        catalog_total: catalog.len(),
        catalog_covered: covered.len(),
        percent,
        covered_modes: covered.iter().cloned().collect(),
        uncovered_modes: uncovered,
        effective_covered_modes: covered.into_iter().collect(),
        declared_but_ineffective_modes: ineffective.into_iter().collect(),
    }
}

pub(crate) fn build_trust_score(
    findings_v2: &[FindingV2],
    validate_ok: bool,
    coverage_percent: f64,
) -> TrustScore {
    let mut critical = 0usize;
    let mut high = 0usize;
    let mut medium = 0usize;
    let mut low = 0usize;
    for f in findings_v2 {
        match f.details.severity {
            FindingSeverity::Critical => critical += 1,
            FindingSeverity::High => high += 1,
            FindingSeverity::Medium => medium += 1,
            FindingSeverity::Low => low += 1,
        }
    }
    let mut score: i32 = 100;
    score -= (critical as i32) * 25;
    score -= (high as i32) * 10;
    score -= (medium as i32) * 4;
    score -= low as i32;
    if !validate_ok {
        score -= 5;
    }
    let coverage_penalty = if coverage_percent < 60.0 {
        ((60.0 - coverage_percent) / 5.0).ceil() as i32
    } else {
        0
    };
    score -= coverage_penalty;
    score = score.clamp(0, 100);
    let grade = if score >= 90 {
        "A"
    } else if score >= 75 {
        "B"
    } else if score >= 60 {
        "C"
    } else if score >= 40 {
        "D"
    } else {
        "F"
    };
    TrustScore {
        score,
        grade: grade.to_string(),
        weights: TrustWeights {
            critical: 25,
            high: 10,
            medium: 4,
            low: 1,
        },
        coverage_penalty,
    }
}

pub(crate) fn compute_weighted_risk(risk: &RiskSummary) -> i32 {
    let mut total = 0i32;
    for (sev, count) in &risk.by_severity {
        let weight = match sev.as_str() {
            "critical" => 25,
            "high" => 10,
            "medium" => 4,
            "low" => 1,
            _ => 1,
        };
        total += (*count as i32) * weight;
    }
    total
}

pub(crate) fn build_quality_posture(
    findings_raw: &[FindingV2],
    coverage: &CoverageSummary,
    risk: &RiskSummary,
) -> QualityPosture {
    let trust = build_trust_score(findings_raw, true, coverage.percent);
    QualityPosture {
        trust_score: trust.score,
        trust_grade: trust.grade,
        coverage_covered: coverage.catalog_covered,
        coverage_total: coverage.catalog_total,
        weighted_risk: compute_weighted_risk(risk),
        findings_total: risk.findings_total,
        risk_by_severity: risk.by_severity.clone(),
    }
}

fn top_violation_codes(violations: &[Violation], limit: usize) -> Vec<String> {
    let mut by_code: BTreeMap<String, usize> = BTreeMap::new();
    for v in violations {
        *by_code.entry(v.code.clone()).or_insert(0) += 1;
    }
    let mut ranked = by_code.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(code, _)| code)
        .collect()
}

pub(crate) fn build_agent_digest(
    decision: &Decision,
    violations: &[Violation],
    findings: &[FindingV2],
) -> AgentDigest {
    build_agent_digest_with_suppressed(decision, violations, findings, &[])
}

pub(crate) fn build_agent_digest_with_suppressed(
    decision: &Decision,
    violations: &[Violation],
    findings: &[FindingV2],
    suppressed: &[Violation],
) -> AgentDigest {
    let mut top_blockers: Vec<String> = decision
        .reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .map(|r| r.code.clone())
        .take(5)
        .collect();
    top_blockers.sort();
    top_blockers.dedup();

    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    for f in findings {
        *by_category.entry(f.details.category.clone()).or_insert(0) += 1;
    }
    let mut root_causes = by_category.into_iter().collect::<Vec<_>>();
    root_causes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let root_causes = root_causes
        .into_iter()
        .take(3)
        .map(|(k, v)| format!("{k} ({v})"))
        .collect::<Vec<_>>();

    let mut minimal_fix_steps: Vec<String> = findings
        .iter()
        .filter_map(|f| f.details.fix_recipe.clone())
        .take(3)
        .collect();
    if minimal_fix_steps.is_empty() && !violations.is_empty() {
        minimal_fix_steps
            .push("Устранить первый blocking violation и повторить validate/gate.".to_string());
    }

    let confidence = if decision
        .reasons
        .iter()
        .any(|r| r.code.starts_with("unknown") || r.code == "unknown")
    {
        "medium"
    } else {
        "high"
    };

    AgentDigest {
        top_blockers,
        root_causes,
        minimal_fix_steps,
        confidence: confidence.to_string(),
        suppressed_count: suppressed.len(),
        suppressed_top_codes: top_violation_codes(suppressed, 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{DecisionReason, DecisionStatus, ErrorClass};

    fn test_decision() -> Decision {
        Decision {
            status: DecisionStatus::Blocked,
            reasons: vec![DecisionReason {
                code: "boundary.rule_violation".to_string(),
                class: ErrorClass::ContractBreak,
                tier: ViolationTier::Blocking,
            }],
            blocking_count: 1,
            observation_count: 0,
        }
    }

    fn test_finding(code: &str, category: &str, fix_recipe: Option<&str>) -> FindingV2 {
        FindingV2 {
            code: code.to_string(),
            message: "msg".to_string(),
            path: None,
            details: FindingDetailsV2 {
                severity: FindingSeverity::High,
                category: category.to_string(),
                confidence: "high".to_string(),
                evidence_refs: vec![],
                fix_recipe: fix_recipe.map(ToString::to_string),
                legacy_details: None,
            },
        }
    }

    #[test]
    fn agent_digest_wrapper_without_suppressed_keeps_defaults() {
        let decision = test_decision();
        let findings = vec![test_finding(
            "finding.boundary.rule_violation",
            "policy_theater",
            Some("Fix boundary"),
        )];
        let digest = build_agent_digest(&decision, &[], &findings);
        assert_eq!(digest.suppressed_count, 0);
        assert!(digest.suppressed_top_codes.is_empty());
    }

    #[test]
    fn agent_digest_with_suppressed_reports_top_codes() {
        let decision = test_decision();
        let findings = vec![test_finding(
            "finding.boundary.rule_violation",
            "policy_theater",
            Some("Fix boundary"),
        )];
        let suppressed = vec![
            Violation::observation("exception.expired", "x", None, None),
            Violation::observation("exception.expired", "x", None, None),
            Violation::observation("loc.max_exceeded", "x", None, None),
            Violation::observation("boundary.rule_violation", "x", None, None),
        ];
        let digest = build_agent_digest_with_suppressed(&decision, &[], &findings, &suppressed);
        assert_eq!(digest.suppressed_count, 4);
        assert_eq!(
            digest.suppressed_top_codes,
            vec![
                "exception.expired".to_string(),
                "boundary.rule_violation".to_string(),
                "loc.max_exceeded".to_string()
            ]
        );
    }
}
