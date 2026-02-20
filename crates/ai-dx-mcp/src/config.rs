use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfig {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub tools: Vec<ProjectTool>,
    #[serde(default)]
    pub(crate) tool_policy: ToolExecutionPolicyConfigV2,
    pub gate: Option<GateConfig>,
    pub checks: Option<ChecksConfigV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMeta {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub tool_import_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTool {
    pub id: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
    pub max_stdout_bytes: Option<usize>,
    pub max_stderr_bytes: Option<usize>,
    #[serde(default)]
    pub receipt_contract: Option<ToolReceiptContract>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionPolicyMode {
    #[default]
    Allowlist,
    AllowAny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolExecutionPolicyConfigV2 {
    #[serde(default)]
    pub mode: ToolExecutionPolicyMode,
    #[serde(default)]
    pub allow_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolReceiptContract {
    pub min_duration_ms: Option<u64>,
    pub min_stdout_bytes: Option<usize>,
    pub expect_stdout_pattern: Option<String>,
    #[serde(default)]
    pub expect_exit_codes: Option<Vec<i32>>,
}

impl Default for ToolExecutionPolicyConfigV2 {
    fn default() -> Self {
        Self {
            mode: ToolExecutionPolicyMode::Allowlist,
            allow_commands: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateConfig {
    #[serde(default)]
    pub ci_fast: Vec<String>,
    #[serde(default)]
    pub ci: Vec<String>,
    #[serde(default)]
    pub flagship: Vec<String>,
}

// --- checks.v2 (multi-instance) ---
//
// NOTE: v2 is intentionally "boring": purely data-driven, deterministic, and fail-closed.
// Runtime wiring + merge semantics are implemented elsewhere (see TASK-008 in BranchMind).

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksConfigV2 {
    #[serde(default)]
    pub loc: Vec<LocCheckConfigV2>,
    #[serde(default)]
    pub env_registry: Vec<EnvRegistryCheckConfigV2>,
    #[serde(default)]
    pub boundary: Vec<BoundaryCheckConfigV2>,
    #[serde(default)]
    pub surface: Vec<SurfaceCheckConfigV2>,
    #[serde(default)]
    pub duplicates: Vec<DuplicatesCheckConfigV2>,
    #[serde(default)]
    pub supply_chain: Vec<SupplyChainCheckConfigV2>,
    #[serde(default)]
    pub tool_budget: Vec<ToolBudgetCheckConfigV2>,
    #[serde(default)]
    pub reuse_first: Vec<ReuseFirstCheckConfigV2>,
    #[serde(default)]
    pub arch_layers: Vec<ArchLayersCheckConfigV2>,
    #[serde(default)]
    pub dead_code: Vec<DeadCodeCheckConfigV2>,
    #[serde(default)]
    pub orphan_api: Vec<OrphanApiCheckConfigV2>,
    #[serde(default)]
    pub complexity_budget: Vec<ComplexityBudgetCheckConfigV2>,
    #[serde(default)]
    pub contract_break: Vec<ContractBreakCheckConfigV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocCheckConfigV2 {
    pub id: String,
    pub max_loc: usize,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    pub baseline_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvRegistryCheckConfigV2 {
    pub id: String,
    pub registry_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub strip_rust_cfg_test_blocks: bool,
    #[serde(default)]
    pub rules: Vec<BoundaryRuleConfigV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryRuleConfigV2 {
    pub id: String,
    pub message: Option<String>,
    pub deny_regex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceCheckConfigV2 {
    pub id: String,
    pub max_items: usize,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub rules: Vec<SurfaceRuleConfigV2>,
    pub baseline_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRuleConfigV2 {
    /// Optional fine-grained file filter for this rule.
    #[serde(default)]
    pub file_globs: Vec<String>,
    pub regex: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicatesCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Only consider files <= this size (bytes). Keep it small to stay fast and deterministic.
    pub max_file_bytes: usize,
    #[serde(default)]
    pub allowlist_globs: Vec<String>,
    pub baseline_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyChainCheckConfigV2 {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBudgetCheckConfigV2 {
    pub id: String,
    pub max_tools_total: usize,
    pub max_tools_per_plugin: usize,
    pub max_gate_tools_per_kind: usize,
    pub max_checks_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReuseFirstCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default = "default_reuse_min_block_lines")]
    pub min_block_lines: usize,
}

const fn default_reuse_min_block_lines() -> usize {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchLayersCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub layers: Vec<ArchLayerConfigV2>,
    #[serde(default)]
    pub rules: Vec<ArchLayerRuleConfigV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchLayerConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub module_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchLayerRuleConfigV2 {
    pub from_layer: String,
    #[serde(default)]
    pub deny_to_layers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeadCodeCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default = "default_min_symbol_len")]
    pub min_symbol_len: usize,
    #[serde(default)]
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrphanApiCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default = "default_min_symbol_len")]
    pub min_symbol_len: usize,
    #[serde(default)]
    pub blocking: bool,
}

const fn default_min_symbol_len() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComplexityBudgetCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    pub max_function_lines: usize,
    pub max_cyclomatic: usize,
    pub max_cognitive: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractBreakCheckConfigV2 {
    pub id: String,
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    pub baseline_path: String,
    #[serde(default = "default_allow_contract_additions")]
    pub allow_additions: bool,
}

const fn default_allow_contract_additions() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct QualityContractConfig {
    #[serde(default)]
    pub quality: QualityThresholds,
    #[serde(default)]
    pub exceptions: ExceptionLimits,
    #[serde(default)]
    pub receipt_defaults: ReceiptDefaults,
    #[serde(default)]
    pub governance: GovernanceConfig,
    #[serde(default)]
    pub baseline: BaselineConfig,
    #[serde(default)]
    pub proof: ProofConfig,
    #[serde(default)]
    pub impact: ImpactConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityThresholds {
    #[serde(default = "default_min_trust_score")]
    pub min_trust_score: i32,
    #[serde(default = "default_min_coverage_percent")]
    pub min_coverage_percent: f64,
    #[serde(default)]
    pub allow_trust_drop: bool,
    #[serde(default)]
    pub allow_coverage_drop: bool,
    #[serde(default)]
    pub max_weighted_risk_increase: i32,
}

const fn default_min_trust_score() -> i32 {
    60
}
const fn default_min_coverage_percent() -> f64 {
    60.0
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            min_trust_score: default_min_trust_score(),
            min_coverage_percent: default_min_coverage_percent(),
            allow_trust_drop: false,
            allow_coverage_drop: false,
            max_weighted_risk_increase: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionLimits {
    #[serde(default = "default_max_exceptions")]
    pub max_exceptions: usize,
    #[serde(default = "default_max_suppressed_ratio")]
    pub max_suppressed_ratio: f64,
    #[serde(default = "default_max_exception_window_days")]
    pub max_exception_window_days: u32,
}

const fn default_max_exceptions() -> usize {
    10
}
const fn default_max_exception_window_days() -> u32 {
    90
}
fn default_max_suppressed_ratio() -> f64 {
    0.30
}

impl Default for ExceptionLimits {
    fn default() -> Self {
        Self {
            max_exceptions: default_max_exceptions(),
            max_suppressed_ratio: default_max_suppressed_ratio(),
            max_exception_window_days: default_max_exception_window_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDefaults {
    #[serde(default = "default_min_duration_ms")]
    pub min_duration_ms: u64,
    #[serde(default = "default_min_stdout_bytes")]
    pub min_stdout_bytes: usize,
}

const fn default_min_duration_ms() -> u64 {
    500
}
const fn default_min_stdout_bytes() -> usize {
    10
}

impl Default for ReceiptDefaults {
    fn default() -> Self {
        Self {
            min_duration_ms: default_min_duration_ms(),
            min_stdout_bytes: default_min_stdout_bytes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceConfig {
    #[serde(default)]
    pub mandatory_checks: Vec<String>,
    #[serde(default)]
    pub mandatory_failure_modes: Vec<String>,
    #[serde(default = "default_min_failure_modes")]
    pub min_failure_modes: usize,
    pub config_hash: Option<String>,
}

const fn default_min_failure_modes() -> usize {
    8
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            mandatory_checks: vec![],
            mandatory_failure_modes: vec![],
            min_failure_modes: default_min_failure_modes(),
            config_hash: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineConfig {
    #[serde(default = "default_snapshot_path")]
    pub snapshot_path: String,
    #[serde(default = "default_max_scope_narrowing")]
    pub max_scope_narrowing: f64,
}

fn default_snapshot_path() -> String {
    ".agents/mcp/compas/baselines/quality_snapshot.json".to_string()
}
fn default_max_scope_narrowing() -> f64 {
    0.10
}

impl Default for BaselineConfig {
    fn default() -> Self {
        Self {
            snapshot_path: default_snapshot_path(),
            max_scope_narrowing: default_max_scope_narrowing(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofConfig {
    #[serde(default = "default_require_witness")]
    pub require_witness: bool,
}

const fn default_require_witness() -> bool {
    true
}

impl Default for ProofConfig {
    fn default() -> Self {
        Self {
            require_witness: default_require_witness(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImpactUnmappedPathPolicy {
    Ignore,
    Observe,
    #[default]
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactRule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path_globs: Vec<String>,
    #[serde(default)]
    pub required_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactConfig {
    #[serde(default = "default_impact_diff_base")]
    pub diff_base: String,
    #[serde(default)]
    pub unmapped_path_policy: ImpactUnmappedPathPolicy,
    #[serde(default)]
    pub rules: Vec<ImpactRule>,
}

fn default_impact_diff_base() -> String {
    "merge-base:origin/main".to_string()
}

impl Default for ImpactConfig {
    fn default() -> Self {
        Self {
            diff_base: default_impact_diff_base(),
            unmapped_path_policy: ImpactUnmappedPathPolicy::default(),
            rules: vec![],
        }
    }
}

#[cfg(test)]
mod tests;
