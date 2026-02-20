use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical tool ids: a small, fixed vocabulary to prevent agents from inventing a zoo of tool names.
///
/// A canonical tool id maps to one or more concrete project tools (by tool_id string).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalToolId {
    Build,
    Test,
    Lint,
    Fmt,
    Docs,
}

/// Canonical tooling wiring for gates and init.
///
/// - Each field is an ordered list of concrete `tool_id`s to execute for that canonical action.
/// - `disabled` is required to distinguish “intentionally off” from “forgot to wire”.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CanonicalToolsConfig {
    #[serde(default)]
    pub build: Vec<String>,
    #[serde(default)]
    pub test: Vec<String>,
    #[serde(default)]
    pub lint: Vec<String>,
    #[serde(default)]
    pub fmt: Vec<String>,
    #[serde(default)]
    pub docs: Vec<String>,
    pub disabled: Vec<CanonicalToolId>,
}
