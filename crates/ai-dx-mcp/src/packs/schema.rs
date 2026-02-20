use serde::{Deserialize, Serialize};

use crate::api::{CanonicalToolId, CanonicalToolsConfig};
use crate::config::{ChecksConfigV2, ProjectTool};

/// `pack.toml` — data-only language pack contract (v1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifestV1 {
    pub pack: PackMetaV1,
    #[serde(default)]
    pub detectors: Vec<PackDetectorV1>,
    #[serde(default)]
    pub tools: Vec<PackToolTemplateV1>,
    pub canonical_tools: Option<CanonicalToolsConfig>,
    pub gates: Option<PackGatesV1>,
    pub checks_v2: Option<ChecksConfigV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackMetaV1 {
    pub id: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
}

/// A boring, deterministic detector: based on file presence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackDetectorV1 {
    pub id: String,
    /// Match if any path in this list exists.
    #[serde(default)]
    pub any_paths: Vec<String>,
    /// Match only if all paths in this list exist.
    #[serde(default)]
    pub all_paths: Vec<String>,
    /// Reject if any path in this list exists.
    #[serde(default)]
    pub none_paths: Vec<String>,
}

/// Tool template is intentionally identical to `tool.toml` schema (nested `tool` table)
/// to avoid inventing a second DSL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackToolTemplateV1 {
    pub tool: ProjectTool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackGatesV1 {
    #[serde(default)]
    pub ci_fast: Vec<CanonicalToolId>,
    #[serde(default)]
    pub ci: Vec<CanonicalToolId>,
    #[serde(default)]
    pub flagship: Vec<CanonicalToolId>,
}

/// `packs.lock` — records pack sources used by init (v1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacksLockV1 {
    pub version: u32,
    #[serde(default)]
    pub packs: Vec<PackLockEntryV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackLockEntryV1 {
    pub id: String,
    pub source: String,
    pub sha256: Option<String>,
    pub resolved_path: Option<String>,
    pub version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_manifest_deserialize_smoke() {
        let s = r#"
[pack]
id = "rust"
version = "0.1.0"
description = "Rust defaults"
languages = ["rust"]

[[detectors]]
id = "cargo"
any_paths = ["Cargo.toml"]

[[tools]]
[tools.tool]
id = "rust-test"
description = "cargo test"
command = "cargo"
args = ["test"]

[canonical_tools]
test = ["rust-test"]
disabled = ["docs", "lint", "fmt", "build"]

[gates]
ci_fast = ["test"]
"#;

        let manifest: PackManifestV1 = toml::from_str(s).expect("deserialize pack.toml");
        assert_eq!(manifest.pack.id, "rust");
        assert_eq!(manifest.detectors.len(), 1);
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.gates.unwrap().ci_fast, vec![CanonicalToolId::Test]);
    }

    #[test]
    fn packs_lock_deserialize_smoke() {
        let s = r#"
version = 1
packs = [
  { id = "rust", source = "builtin:rust", version = "0.1.0" },
  { id = "org/custom", source = "https://example.com/pack.tar.gz", sha256 = "00".repeat(32), resolved_path = ".agents/mcp/compas/packs/vendor/...", version = "1.2.3" },
]
"#;

        // toml doesn't support `.repeat()` in strings, so we keep it simple here.
        let s = s.replace("\"00\".repeat(32)", &format!("\"{}\"", "00".repeat(32)));
        let lock: PacksLockV1 = toml::from_str(&s).expect("deserialize packs.lock");
        assert_eq!(lock.version, 1);
        assert_eq!(lock.packs.len(), 2);
        let sha = "00".repeat(32);
        assert_eq!(lock.packs[1].sha256.as_deref(), Some(sha.as_str()));
    }
}
