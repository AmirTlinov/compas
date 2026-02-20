use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindingDetailsV2 {
    pub severity: FindingSeverity,
    pub category: String,
    pub confidence: String,
    pub evidence_refs: Vec<String>,
    pub fix_recipe: Option<String>,
    pub legacy_details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindingV2 {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub details: FindingDetailsV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RiskSummary {
    pub findings_total: usize,
    pub by_category: BTreeMap<String, usize>,
    pub by_severity: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CoverageSummary {
    pub catalog_total: usize,
    pub catalog_covered: usize,
    pub percent: f64,
    pub covered_modes: Vec<String>,
    pub uncovered_modes: Vec<String>,
    #[serde(default)]
    pub effective_covered_modes: Vec<String>,
    #[serde(default)]
    pub declared_but_ineffective_modes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrustWeights {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrustScore {
    pub score: i32,
    pub grade: String,
    pub weights: TrustWeights,
    #[serde(default)]
    pub coverage_penalty: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentDigest {
    pub top_blockers: Vec<String>,
    pub root_causes: Vec<String>,
    pub minimal_fix_steps: Vec<String>,
    pub confidence: String,
    #[serde(default)]
    pub suppressed_count: usize,
    #[serde(default)]
    pub suppressed_top_codes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_digest_back_compat_defaults_for_suppressed_summary() {
        let digest: AgentDigest = serde_json::from_value(serde_json::json!({
            "top_blockers": ["boundary.rule_violation"],
            "root_causes": ["policy_theater (1)"],
            "minimal_fix_steps": ["Fix boundary rule"],
            "confidence": "high"
        }))
        .expect("deserialize legacy AgentDigest payload");
        assert_eq!(digest.suppressed_count, 0);
        assert!(digest.suppressed_top_codes.is_empty());
    }
}
