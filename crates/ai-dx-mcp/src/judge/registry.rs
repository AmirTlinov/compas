use crate::api::{ErrorClass, ViolationTier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViolationClassEntry {
    pub pattern: ViolationPattern,
    pub class: ErrorClass,
    pub tier: ViolationTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationPattern {
    Exact(&'static str),
    Prefix(&'static str),
    Suffix(&'static str),
}

const fn entry(
    pattern: ViolationPattern,
    class: ErrorClass,
    tier: ViolationTier,
) -> ViolationClassEntry {
    ViolationClassEntry {
        pattern,
        class,
        tier,
    }
}

use ErrorClass::*;
use ViolationPattern::*;
use ViolationTier::*;

/// Единственный реестр классификации violation-кодов.
/// Порядок важен внутри одинакового типа паттерна (см. classify()).
pub static VIOLATION_REGISTRY: &[ViolationClassEntry] = &[
    // Infrastructure failures (suffix priority)
    entry(Suffix(".check_failed"), RuntimeRisk, Blocking),
    entry(Suffix(".read_failed"), RuntimeRisk, Blocking),
    entry(Suffix(".stat_failed"), RuntimeRisk, Blocking),
    entry(Suffix(".manifest_parse_failed"), RuntimeRisk, Blocking),
    // Config / structural
    entry(Prefix("config."), SchemaConfig, Blocking),
    entry(Prefix("failure_modes."), SchemaConfig, Blocking),
    entry(Prefix("pack."), SchemaConfig, Blocking),
    entry(Exact("exception.allowlist_invalid"), SchemaConfig, Blocking),
    // Security
    entry(Prefix("supply_chain."), Security, Blocking),
    entry(Exact("security.allow_any_policy"), Security, Blocking),
    // Unified ratchet
    entry(Prefix("quality_delta."), QualityRegression, Blocking),
    // Policy / contract
    entry(Prefix("boundary."), ContractBreak, Blocking),
    entry(Exact("exception.expired"), ContractBreak, Blocking),
    entry(Exact("exception.window_exceeded"), ContractBreak, Blocking),
    entry(Exact("exception.budget_exceeded"), ContractBreak, Blocking),
    entry(Prefix("tools.duplicate_exact"), ContractBreak, Blocking),
    entry(
        Prefix("tools.duplicate_semantic"),
        ContractBreak,
        Observation,
    ),
    // Observations
    entry(Prefix("loc."), ContractBreak, Observation),
    entry(Prefix("surface."), ContractBreak, Observation),
    entry(Prefix("duplicates."), ContractBreak, Observation),
    entry(Prefix("dead_code."), ContractBreak, Observation),
    entry(Prefix("orphan_api."), ContractBreak, Observation),
    entry(Prefix("env_registry."), ContractBreak, Observation),
    entry(Prefix("tool_budget."), ContractBreak, Observation),
    // Architecture / reuse / complexity / contracts
    entry(Prefix("reuse_first."), ContractBreak, Blocking),
    entry(Prefix("arch_layers."), ContractBreak, Blocking),
    entry(Prefix("complexity_budget."), ContractBreak, Blocking),
    entry(Prefix("contract_break."), ContractBreak, Blocking),
    entry(Prefix("change_impact."), ContractBreak, Blocking),
    // Gate execution
    entry(Prefix("gate.receipt_contract"), RuntimeRisk, Blocking),
    entry(Prefix("gate.tool_failed"), ContractBreak, Blocking),
    entry(Exact("gate.run_failed_transient"), TransientTool, Blocking),
    entry(Prefix("gate.run_failed"), RuntimeRisk, Blocking),
    entry(Prefix("gate.observation."), ContractBreak, Observation),
    entry(Prefix("gate."), SchemaConfig, Blocking),
    entry(Prefix("witness."), RuntimeRisk, Blocking),
];

pub fn classify(code: &str) -> (ErrorClass, ViolationTier) {
    // 1) suffix (most specific)
    for item in VIOLATION_REGISTRY {
        if let ViolationPattern::Suffix(s) = item.pattern
            && code.ends_with(s)
        {
            return (item.class, item.tier);
        }
    }
    // 2) exact
    for item in VIOLATION_REGISTRY {
        if let ViolationPattern::Exact(s) = item.pattern
            && code == s
        {
            return (item.class, item.tier);
        }
    }
    // 3) prefix
    for item in VIOLATION_REGISTRY {
        if let ViolationPattern::Prefix(s) = item.pattern
            && code.starts_with(s)
        {
            return (item.class, item.tier);
        }
    }
    (ErrorClass::Unknown, ViolationTier::Blocking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_code_is_unknown_blocking() {
        let (class, tier) = classify("something.never.seen");
        assert_eq!(class, ErrorClass::Unknown);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn suffix_has_priority_over_prefix() {
        let (class, tier) = classify("loc.read_failed");
        assert_eq!(class, ErrorClass::RuntimeRisk);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn observation_tier_for_loc_surface_duplicates() {
        for code in [
            "loc.max_exceeded",
            "surface.max_exceeded",
            "duplicates.found",
            "gate.observation.trace",
        ] {
            let (_class, tier) = classify(code);
            assert_eq!(tier, ViolationTier::Observation, "{code}");
        }
    }

    #[test]
    fn quality_delta_is_blocking() {
        let (class, tier) = classify("quality_delta.trust_regression");
        assert_eq!(class, ErrorClass::QualityRegression);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn gate_run_failed_transient_is_retryable_class() {
        let (class, tier) = classify("gate.run_failed_transient");
        assert_eq!(class, ErrorClass::TransientTool);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn gate_run_failed_non_transient_is_runtime_risk() {
        let (class, tier) = classify("gate.run_failed");
        assert_eq!(class, ErrorClass::RuntimeRisk);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn exception_window_exceeded_is_contract_break_blocking() {
        let (class, tier) = classify("exception.window_exceeded");
        assert_eq!(class, ErrorClass::ContractBreak);
        assert_eq!(tier, ViolationTier::Blocking);
    }
}
