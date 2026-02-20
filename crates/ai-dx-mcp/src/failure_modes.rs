use regex::Regex;
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

const REL_FAILURE_MODES_PATH: &str = ".agents/mcp/compas/failure_modes.toml";

const DEFAULT_CATALOG: [&str; 10] = [
    "policy_theater",
    "unplugged_iron",
    "fail_open",
    "env_sprawl",
    "public_surface_bloat",
    "god_module_cycles",
    "resilience_defaults",
    "security_baseline",
    "dependency_hygiene",
    "knowledge_continuity",
];

fn mode_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{1,63}$").expect("static regex is valid"))
}

#[derive(Debug)]
pub(crate) struct FailureModesError {
    pub path: PathBuf,
    pub message: String,
}

impl std::fmt::Display for FailureModesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (path={})", self.message, self.path.display())
    }
}

impl std::error::Error for FailureModesError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureModesFile {
    catalog: Vec<String>,
}

pub(crate) fn default_failure_mode_catalog() -> Vec<String> {
    DEFAULT_CATALOG.iter().map(|s| s.to_string()).collect()
}

pub(crate) fn failure_modes_path(repo_root: &Path) -> PathBuf {
    repo_root.join(REL_FAILURE_MODES_PATH)
}

pub(crate) fn load_failure_mode_catalog(
    repo_root: &Path,
) -> Result<Vec<String>, FailureModesError> {
    let path = failure_modes_path(repo_root);
    if !path.exists() {
        return Ok(default_failure_mode_catalog());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| FailureModesError {
        path: path.clone(),
        message: format!("failed to read failure mode catalog: {e}"),
    })?;
    let parsed: FailureModesFile = toml::from_str(&raw).map_err(|e| FailureModesError {
        path: path.clone(),
        message: format!("invalid failure mode catalog TOML: {e}"),
    })?;
    validate_catalog(parsed.catalog, &path)
}

fn validate_catalog(catalog: Vec<String>, path: &Path) -> Result<Vec<String>, FailureModesError> {
    if catalog.is_empty() {
        return Err(FailureModesError {
            path: path.to_path_buf(),
            message: "failure mode catalog must not be empty".to_string(),
        });
    }
    let re = mode_id_regex();
    let mut out: Vec<String> = Vec::with_capacity(catalog.len());
    let mut seen = std::collections::BTreeSet::new();
    for raw in catalog {
        let id = raw.trim().to_string();
        if id.is_empty() {
            return Err(FailureModesError {
                path: path.to_path_buf(),
                message: "failure mode catalog contains empty id".to_string(),
            });
        }
        if !re.is_match(&id) {
            return Err(FailureModesError {
                path: path.to_path_buf(),
                message: format!("invalid failure mode id '{id}'"),
            });
        }
        if !seen.insert(id.clone()) {
            return Err(FailureModesError {
                path: path.to_path_buf(),
                message: format!("duplicate failure mode id '{id}'"),
            });
        }
        out.push(id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_file_uses_default_catalog() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let got = load_failure_mode_catalog(tmp.path()).expect("load default");
        assert_eq!(got, default_failure_mode_catalog());
    }

    #[test]
    fn valid_file_is_loaded() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = failure_modes_path(tmp.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(
            &path,
            r#"
catalog = ["policy_theater", "unplugged_iron", "fail_open"]
"#,
        )
        .expect("write");
        let got = load_failure_mode_catalog(tmp.path()).expect("load custom");
        assert_eq!(
            got,
            vec![
                "policy_theater".to_string(),
                "unplugged_iron".to_string(),
                "fail_open".to_string()
            ]
        );
    }

    #[test]
    fn invalid_file_fails_closed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = failure_modes_path(tmp.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(
            &path,
            r#"
catalog = ["policy_theater", "policy_theater"]
"#,
        )
        .expect("write");
        let err = load_failure_mode_catalog(tmp.path()).expect_err("must fail");
        assert!(err.message.contains("duplicate failure mode id"));
        assert!(err.path.ends_with(".agents/mcp/compas/failure_modes.toml"));
    }
}
