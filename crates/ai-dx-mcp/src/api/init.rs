use super::{ApiError, PayloadMeta};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitRequest {
    pub repo_root: Option<String>,
    pub apply: Option<bool>,
    pub profile: Option<String>,
    #[serde(default)]
    pub registry_source: Option<String>,
    /// Optional pack selection override (e.g., ["builtin:rust", "builtin:node"]).
    pub packs: Option<Vec<String>>,
    /// Optional external packs (pinned by sha256). Download is allowed only during init.
    pub external_packs: Option<Vec<ExternalPackRef>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackRef {
    pub source: String,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InitWriteFile {
    pub path: String,
    pub content_utf8: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InitPlan {
    #[serde(default)]
    pub writes: Vec<InitWriteFile>,
    #[serde(default)]
    pub deletes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct InitRecommendations {
    #[serde(default)]
    pub recommended: Vec<InitRegistryPackRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitRegistryPackRecommendation {
    pub pack_id: String,
    pub why: String,
    pub cost_class: String,
    pub runtime_kind: String,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    #[serde(default)]
    pub warnings: Vec<ApiError>,
    pub repo_root: String,
    pub applied: bool,
    pub plan: Option<InitPlan>,
    #[serde(default)]
    pub recommendations: Option<InitRecommendations>,
    #[serde(default)]
    pub summary_md: Option<String>,
    #[serde(default)]
    pub payload_meta: Option<PayloadMeta>,
}
