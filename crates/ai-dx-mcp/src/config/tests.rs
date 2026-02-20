use super::*;

#[test]
fn checks_v2_deserialize_smoke() {
    let s = r#"
loc = [{ id = "loc-main", max_loc = 123, baseline_path = ".agents/loc.json" }]
env_registry = [{ id = "env", registry_path = ".agents/env.toml" }]
boundary = [{ id = "boundary", rules = [{ id = "b1", deny_regex = "forbidden" }] }]
surface = [{ id = "surface", max_items = 10, baseline_path = ".agents/surface.json", rules = [{ regex = "pub\\s+fn" }] }]
duplicates = [{ id = "dup", max_file_bytes = 8192, baseline_path = ".agents/dup.json" }]
supply_chain = [{ id = "supply-chain" }]
tool_budget = [{ id = "tool-budget", max_tools_total = 20, max_tools_per_plugin = 10, max_gate_tools_per_kind = 6, max_checks_total = 12 }]
reuse_first = [{ id = "reuse", min_block_lines = 6 }]
arch_layers = [{ id = "layers", layers = [{ id = "core", include_globs = ["src/core/**"], module_prefixes = ["core"] }], rules = [{ from_layer = "core", deny_to_layers = ["ui"] }] }]
dead_code = [{ id = "dead", min_symbol_len = 3 }]
orphan_api = [{ id = "orphan", min_symbol_len = 3 }]
complexity_budget = [{ id = "complex", max_function_lines = 80, max_cyclomatic = 15, max_cognitive = 20 }]
contract_break = [{ id = "contract", baseline_path = ".agents/mcp/compas/baselines/contracts.json" }]
"#;

    let cfg: ChecksConfigV2 = toml::from_str(s).expect("deserialize ChecksConfigV2");
    assert_eq!(cfg.loc.len(), 1);
    assert_eq!(cfg.env_registry.len(), 1);
    assert_eq!(cfg.boundary.len(), 1);
    assert_eq!(cfg.surface.len(), 1);
    assert_eq!(cfg.duplicates.len(), 1);
    assert_eq!(cfg.supply_chain.len(), 1);
    assert_eq!(cfg.tool_budget.len(), 1);
    assert_eq!(cfg.reuse_first.len(), 1);
    assert_eq!(cfg.arch_layers.len(), 1);
    assert_eq!(cfg.dead_code.len(), 1);
    assert_eq!(cfg.orphan_api.len(), 1);
    assert_eq!(cfg.complexity_budget.len(), 1);
    assert_eq!(cfg.contract_break.len(), 1);
    assert_eq!(cfg.duplicates[0].max_file_bytes, 8192);
}

#[test]
fn quality_contract_deserialize() {
    let s = r#"
[quality]
min_trust_score = 60
min_coverage_percent = 60.0
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

[exceptions]
max_exceptions = 10
max_suppressed_ratio = 0.30
max_exception_window_days = 90

[receipt_defaults]
min_duration_ms = 500
min_stdout_bytes = 10

[governance]
mandatory_checks = ["boundary", "supply_chain"]
mandatory_failure_modes = ["security_baseline", "resilience_defaults"]
min_failure_modes = 8

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10

[impact]
diff_base = "merge-base:origin/main"
unmapped_path_policy = "block"
"#;
    let cfg: QualityContractConfig = toml::from_str(s).expect("parse quality_contract");
    assert_eq!(cfg.quality.min_trust_score, 60);
    assert!((cfg.quality.min_coverage_percent - 60.0).abs() < f64::EPSILON);
    assert!(!cfg.quality.allow_trust_drop);
    assert_eq!(
        cfg.governance.mandatory_checks,
        vec!["boundary", "supply_chain"]
    );
    assert!((cfg.baseline.max_scope_narrowing - 0.10).abs() < f64::EPSILON);
    assert_eq!(cfg.impact.diff_base, "merge-base:origin/main");
    assert_eq!(
        cfg.impact.unmapped_path_policy,
        ImpactUnmappedPathPolicy::Block
    );
}

#[test]
fn quality_contract_coverage_minimum_has_safe_default_when_omitted() {
    let s = r#"
[quality]
min_trust_score = 60
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0
"#;
    let cfg: QualityContractConfig = toml::from_str(s).expect("parse quality_contract");
    assert!(
        (cfg.quality.min_coverage_percent - 60.0).abs() < f64::EPSILON,
        "min_coverage_percent default must stay conservative for stable templates"
    );
}

#[test]
fn impact_unmapped_path_policy_deserialize_supports_observe_without_breaking_existing_values() {
    #[derive(serde::Deserialize)]
    struct Probe {
        unmapped_path_policy: ImpactUnmappedPathPolicy,
    }

    let block: Probe = toml::from_str(r#"unmapped_path_policy = "block""#).expect("block");
    let ignore: Probe = toml::from_str(r#"unmapped_path_policy = "ignore""#).expect("ignore");
    let observe: Probe = toml::from_str(r#"unmapped_path_policy = "observe""#).expect("observe");

    assert_eq!(block.unmapped_path_policy, ImpactUnmappedPathPolicy::Block);
    assert_eq!(
        ignore.unmapped_path_policy,
        ImpactUnmappedPathPolicy::Ignore
    );
    assert_eq!(
        observe.unmapped_path_policy,
        ImpactUnmappedPathPolicy::Observe
    );
}
