use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ApiError, PayloadMeta};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitRequest {
    pub repo_root: Option<String>,
    /// When true, applies the generated plan to the repo. Default: false (dry-run).
    pub apply: Option<bool>,
    /// Optional pack selection override (e.g., ["builtin:rust", "builtin:node"]).
    pub packs: Option<Vec<String>>,
    /// Optional external packs (pinned by sha256). Download is allowed only during init.
    pub external_packs: Option<Vec<ExternalPackRef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackRef {
    /// Examples: "https://…/pack.tar.gz", "file:/abs/path/to/pack".
    pub source: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitWriteFile {
    pub path: String,
    pub content_utf8: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct InitPlan {
    #[serde(default)]
    pub writes: Vec<InitWriteFile>,
    #[serde(default)]
    pub deletes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InitOutput {
    pub ok: bool,
    pub error: Option<ApiError>,
    pub repo_root: String,
    pub applied: bool,
    pub plan: Option<InitPlan>,
    #[serde(default)]
    pub summary_md: Option<String>,
    #[serde(default)]
    pub payload_meta: Option<PayloadMeta>,
}
