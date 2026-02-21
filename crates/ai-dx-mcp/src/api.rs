use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod canonical;
mod init;
mod insights;

pub use canonical::{CanonicalToolId, CanonicalToolsConfig};
pub use init::{ExternalPackRef, InitOutput, InitPlan, InitRequest, InitWriteFile};
pub use insights::{
    AgentDigest, CoverageSummary, FindingDetailsV2, FindingSeverity, FindingV2, RiskSummary,
    TrustScore, TrustWeights,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Violation {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub details: Option<serde_json::Value>,
    #[serde(default)]
    pub tier: ViolationTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidateMode {
    Ratchet,
    Strict,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    #[default]
    Compact,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct PayloadMeta {
    pub mode: ResponseMode,
    pub truncated: bool,
    #[serde(default)]
    pub omitted: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViolationTier {
    #[default]
    Blocking,
    Observation,
}

impl Violation {
    pub fn blocking(
        code: impl Into<String>,
        message: impl Into<String>,
        path: Option<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path,
            details,
            tier: ViolationTier::Blocking,
        }
    }

    pub fn observation(
        code: impl Into<String>,
        message: impl Into<String>,
        path: Option<String>,
        details: Option<serde_json::Value>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            path,
            details,
            tier: ViolationTier::Observation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Pass,
    Retryable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    SchemaConfig,
    ContractBreak,
    RuntimeRisk,
    Security,
    QualityRegression,
    TransientTool,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecisionReason {
    pub code: String,
    pub class: ErrorClass,
    pub tier: ViolationTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Decision {
    pub status: DecisionStatus,
    pub reasons: Vec<DecisionReason>,
    pub blocking_count: usize,
    pub observation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualityPosture {
    pub trust_score: i32,
    pub trust_grade: String,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Verdict {
    pub decision: Decision,
    #[serde(default)]
    pub quality_posture: Option<QualityPosture>,
    #[serde(default)]
    pub suppressed_count: usize,
    #[serde(default)]
    pub suppressed_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BaselineMaintenance {
    pub reason: String,
    pub owner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateRequest {
    pub repo_root: Option<String>,
    pub mode: ValidateMode,
    /// When true, writes/updates baseline files for enabled checks.
    pub write_baseline: Option<bool>,
    #[serde(default)]
    pub baseline_maintenance: Option<BaselineMaintenance>,
    #[serde(default)]
    pub response_mode: Option<ResponseMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LocSummary {
    pub files_scanned: usize,
    pub max_loc: usize,
    pub worst_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BoundarySummary {
    pub files_scanned: usize,
    pub rules_checked: usize,
    pub violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PublicSurfaceSummary {
    pub baseline_path: String,
    pub max_pub_items: usize,
    pub items_total: usize,
    pub added_vs_baseline: usize,
    pub removed_vs_baseline: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveConfigSource {
    Env,
    Default,
    Unset,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EffectiveConfigEntry {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub sensitive: bool,
    pub source: EffectiveConfigSource,
    pub value: Option<String>,
    pub used_by_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EffectiveConfigSummary {
    pub registry_path: String,
    pub registered_vars: usize,
    pub used_vars: Vec<String>,
    pub entries: Vec<EffectiveConfigEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidateOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    /// CIM schema version for downstream parsers.
    pub schema_version: String,
    pub repo_root: String,
    pub mode: ValidateMode,
    pub violations: Vec<Violation>,
    /// CIM: normalized findings with severity/category/fix hints in `details`.
    pub findings_v2: Vec<FindingV2>,
    pub suppressed: Vec<Violation>,
    pub loc: Option<LocSummary>,
    pub boundary: Option<BoundarySummary>,
    pub public_surface: Option<PublicSurfaceSummary>,
    pub effective_config: Option<EffectiveConfigSummary>,
    /// CIM: aggregated risk counts by severity/category.
    pub risk_summary: Option<RiskSummary>,
    /// CIM: coverage against canonical failure-mode catalog.
    pub coverage: Option<CoverageSummary>,
    /// CIM: trust posture derived from current findings.
    pub trust_score: Option<TrustScore>,
    /// Judge verdict (pass/retryable/blocked).
    pub verdict: Option<Verdict>,
    /// Raw (pre-suppress) quality posture for ratchet/quality_delta.
    pub quality_posture: Option<QualityPosture>,
    /// Agent-first compact diagnosis & minimal fix plan.
    pub agent_digest: Option<AgentDigest>,
    #[serde(default)]
    pub summary_md: Option<String>,
    #[serde(default)]
    pub payload_meta: Option<PayloadMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsListRequest {
    pub repo_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectToolInfo {
    pub id: String,
    pub plugin_id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolsListOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    pub repo_root: String,
    pub tools: Vec<ProjectToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsDescribeRequest {
    pub repo_root: Option<String>,
    pub tool_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectToolSpec {
    pub id: String,
    pub plugin_id: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PluginInfo {
    pub id: String,
    pub description: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PluginSpec {
    pub id: String,
    pub description: String,
    pub tools: Vec<ProjectToolSpec>,
    pub gate_ci_fast: Vec<String>,
    pub gate_ci: Vec<String>,
    pub gate_flagship: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolsDescribeOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    pub repo_root: String,
    pub tool: Option<ProjectToolSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsRunRequest {
    pub repo_root: Option<String>,
    pub tool_id: String,
    pub args: Option<Vec<String>>,
    pub dry_run: Option<bool>,
    #[serde(default)]
    pub response_mode: Option<ResponseMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Receipt {
    pub tool_id: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub command: String,
    pub args: Vec<String>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    #[serde(default)]
    pub structured_report: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolsRunOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    pub repo_root: String,
    pub receipt: Option<Receipt>,
    #[serde(default)]
    pub summary_md: Option<String>,
    #[serde(default)]
    pub payload_meta: Option<PayloadMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    #[serde(alias = "ci-fast")]
    CiFast,
    Ci,
    Flagship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateOp {
    Run,
    Start,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateJobState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobInfo {
    pub job_id: String,
    pub state: GateJobState,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GateRequest {
    pub repo_root: Option<String>,
    pub kind: GateKind,
    pub dry_run: Option<bool>,
    pub write_witness: Option<bool>,
    #[serde(default)]
    pub op: Option<GateOp>,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
    #[serde(default)]
    pub response_mode: Option<ResponseMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WitnessMeta {
    pub path: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub rotated_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GateOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    pub repo_root: String,
    pub kind: GateKind,
    pub validate: ValidateOutput,
    pub receipts: Vec<Receipt>,
    pub witness_path: Option<String>,
    pub witness: Option<WitnessMeta>,
    /// Gate-level verdict (includes validate + gate execution reasons).
    pub verdict: Option<Verdict>,
    /// Agent-first compact diagnosis & minimal fix plan.
    pub agent_digest: Option<AgentDigest>,
    #[serde(default)]
    pub summary_md: Option<String>,
    #[serde(default)]
    pub payload_meta: Option<PayloadMeta>,
    #[serde(default)]
    pub job: Option<JobInfo>,
    #[serde(default)]
    pub job_state: Option<GateJobState>,
    #[serde(default)]
    pub job_error: Option<ApiError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_kind_accepts_ci_fast_hyphen_alias() {
        let req: GateRequest = serde_json::from_value(serde_json::json!({
            "repo_root": ".",
            "kind": "ci-fast",
            "dry_run": true,
            "write_witness": false
        }))
        .expect("deserialize GateRequest");

        assert_eq!(req.kind, GateKind::CiFast);
    }

    #[test]
    fn validate_request_rejects_unknown_fields() {
        let err = serde_json::from_value::<ValidateRequest>(serde_json::json!({
            "repo_root": ".",
            "mode": "ratchet",
            "unknown": 1
        }))
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn violation_tier_default_is_blocking() {
        let v: Violation = serde_json::from_value(serde_json::json!({
            "code": "test.x",
            "message": "msg"
        }))
        .expect("deserialize Violation without tier");
        assert_eq!(v.tier, ViolationTier::Blocking);
    }

    #[test]
    fn violation_tier_roundtrip() {
        let v = Violation::observation("test.x", "msg", None, None);
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["tier"], "observation");
        let back: Violation = serde_json::from_value(json).unwrap();
        assert_eq!(back.tier, ViolationTier::Observation);
    }

    #[test]
    fn verdict_roundtrip() {
        let v = Verdict {
            decision: Decision {
                status: DecisionStatus::Blocked,
                reasons: vec![DecisionReason {
                    code: "boundary.rule_violation".to_string(),
                    class: ErrorClass::ContractBreak,
                    tier: ViolationTier::Blocking,
                }],
                blocking_count: 1,
                observation_count: 0,
            },
            quality_posture: Some(QualityPosture {
                trust_score: 72,
                trust_grade: "C".to_string(),
                coverage_covered: 8,
                coverage_total: 12,
                weighted_risk: 34,
                findings_total: 5,
                risk_by_severity: [("high".to_string(), 2), ("medium".to_string(), 3)]
                    .into_iter()
                    .collect(),
            }),
            suppressed_count: 2,
            suppressed_codes: vec![
                "exception.expired".to_string(),
                "boundary.rule_violation".to_string(),
            ],
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["decision"]["status"], "blocked");
        assert_eq!(json["suppressed_count"], 2);
        let back: Verdict = serde_json::from_value(json).unwrap();
        assert_eq!(back.decision.status, DecisionStatus::Blocked);
        assert_eq!(back.suppressed_count, 2);
        assert_eq!(back.suppressed_codes.len(), 2);
    }

    #[test]
    fn verdict_back_compat_defaults_for_new_fields() {
        let back: Verdict = serde_json::from_value(serde_json::json!({
            "decision": {
                "status": "pass",
                "reasons": [],
                "blocking_count": 0,
                "observation_count": 0
            }
        }))
        .expect("deserialize legacy Verdict payload");
        assert!(back.quality_posture.is_none());
        assert_eq!(back.suppressed_count, 0);
        assert!(back.suppressed_codes.is_empty());
    }

    #[test]
    fn quality_posture_roundtrip() {
        let qp = QualityPosture {
            trust_score: 85,
            trust_grade: "B".to_string(),
            coverage_covered: 8,
            coverage_total: 10,
            weighted_risk: 12,
            findings_total: 3,
            risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)]
                .into_iter()
                .collect(),
        };
        let json = serde_json::to_value(&qp).unwrap();
        let back: QualityPosture = serde_json::from_value(json).unwrap();
        assert_eq!(back.trust_score, 85);
    }

    #[test]
    fn validate_request_with_baseline_maintenance() {
        let req: ValidateRequest = serde_json::from_value(serde_json::json!({
            "mode": "ratchet",
            "write_baseline": true,
            "baseline_maintenance": {
                "reason": "Quarterly baseline refresh after major refactor",
                "owner": "team-lead"
            }
        }))
        .expect("deserialize with baseline_maintenance");
        assert!(req.baseline_maintenance.is_some());
        let bm = req.baseline_maintenance.unwrap();
        assert_eq!(bm.owner, "team-lead");
    }

    #[test]
    fn validate_request_without_baseline_maintenance_still_works() {
        let req: ValidateRequest = serde_json::from_value(serde_json::json!({
            "mode": "warn"
        }))
        .expect("deserialize without baseline_maintenance");
        assert!(req.baseline_maintenance.is_none());
    }

    #[test]
    fn init_request_roundtrip_smoke() {
        let req = InitRequest {
            repo_root: Some(".".to_string()),
            apply: Some(false),
            packs: Some(vec!["builtin:rust".to_string()]),
            external_packs: Some(vec![ExternalPackRef {
                source: "file:/tmp/pack".to_string(),
                sha256: "00".repeat(32),
            }]),
        };

        let v = serde_json::to_value(&req).expect("serialize InitRequest");
        let parsed: InitRequest = serde_json::from_value(v).expect("deserialize InitRequest");
        assert_eq!(parsed.repo_root.as_deref(), Some("."));
    }

    #[test]
    fn validate_request_response_mode_roundtrip() {
        let req: ValidateRequest = serde_json::from_value(serde_json::json!({
            "mode": "ratchet",
            "response_mode": "full"
        }))
        .expect("deserialize ValidateRequest");
        assert_eq!(req.response_mode, Some(ResponseMode::Full));
    }

    #[test]
    fn gate_request_status_mode_parses() {
        let req: GateRequest = serde_json::from_value(serde_json::json!({
            "kind": "ci_fast",
            "op": "status",
            "job_id": "gate-1",
            "wait_ms": 2500
        }))
        .expect("deserialize GateRequest status");
        assert_eq!(req.op, Some(GateOp::Status));
        assert_eq!(req.job_id.as_deref(), Some("gate-1"));
        assert_eq!(req.wait_ms, Some(2500));
    }
}
