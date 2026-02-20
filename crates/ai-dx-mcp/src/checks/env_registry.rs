use crate::api::{EffectiveConfigEntry, EffectiveConfigSource, EffectiveConfigSummary, Violation};
use crate::config::{EnvRegistryCheckConfigV2, ProjectTool};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct EnvRegistryResult {
    pub violations: Vec<Violation>,
    pub summary: EffectiveConfigSummary,
}

#[derive(Debug, Deserialize)]
struct EnvRegistryFile {
    #[serde(default)]
    vars: Vec<EnvVarSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct EnvVarSpec {
    name: String,
    description: Option<String>,
    #[serde(default)]
    required: bool,
    default: Option<String>,
    #[serde(default)]
    sensitive: bool,
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_uppercase() {
        return false;
    }

    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn redact_value(raw: String, sensitive: bool) -> String {
    if sensitive {
        "<redacted>".to_string()
    } else {
        raw
    }
}

fn mk_violation(
    code: &str,
    message: String,
    path: Option<String>,
    details: Option<serde_json::Value>,
) -> Violation {
    Violation::observation(code, message, path, details)
}

fn collect_tool_env_usage(
    tools: &BTreeMap<String, ProjectTool>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut usage: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (tool_id, tool) in tools {
        for env_name in tool.env.keys() {
            usage
                .entry(env_name.clone())
                .or_default()
                .insert(tool_id.clone());
        }
    }
    usage
}

fn empty_summary(cfg: &EnvRegistryCheckConfigV2, used_vars: Vec<String>) -> EffectiveConfigSummary {
    EffectiveConfigSummary {
        registry_path: cfg.registry_path.clone(),
        registered_vars: 0,
        used_vars,
        entries: vec![],
    }
}

pub fn run_env_registry_check(
    repo_root: &Path,
    cfg: &EnvRegistryCheckConfigV2,
    tools: &BTreeMap<String, ProjectTool>,
) -> EnvRegistryResult {
    let usage = collect_tool_env_usage(tools);
    let used_vars: Vec<String> = usage.keys().cloned().collect();
    let registry_abs = repo_root.join(&cfg.registry_path);

    if !registry_abs.is_file() {
        return EnvRegistryResult {
            violations: vec![mk_violation(
                "env_registry.registry_missing",
                format!("env registry file is missing: {:?}", registry_abs),
                Some(cfg.registry_path.clone()),
                None,
            )],
            summary: empty_summary(cfg, used_vars),
        };
    }

    let raw = match std::fs::read_to_string(&registry_abs) {
        Ok(v) => v,
        Err(e) => {
            return EnvRegistryResult {
                violations: vec![mk_violation(
                    "env_registry.registry_invalid",
                    format!("failed to read env registry {:?}: {e}", registry_abs),
                    Some(cfg.registry_path.clone()),
                    None,
                )],
                summary: empty_summary(cfg, used_vars),
            };
        }
    };

    let parsed: EnvRegistryFile = match toml::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return EnvRegistryResult {
                violations: vec![mk_violation(
                    "env_registry.registry_invalid",
                    format!("failed to parse env registry {:?}: {e}", registry_abs),
                    Some(cfg.registry_path.clone()),
                    None,
                )],
                summary: empty_summary(cfg, used_vars),
            };
        }
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut specs: Vec<EnvVarSpec> = vec![];

    for mut spec in parsed.vars {
        spec.name = spec.name.trim().to_string();
        spec.description = spec.description.map(|d| d.trim().to_string());

        if spec.name.is_empty() {
            return EnvRegistryResult {
                violations: vec![mk_violation(
                    "env_registry.registry_invalid",
                    "env registry entry has empty name".to_string(),
                    Some(cfg.registry_path.clone()),
                    None,
                )],
                summary: empty_summary(cfg, used_vars),
            };
        }

        if !is_valid_env_name(&spec.name) {
            return EnvRegistryResult {
                violations: vec![mk_violation(
                    "env_registry.registry_invalid",
                    format!("invalid env var name in registry: {}", spec.name),
                    Some(cfg.registry_path.clone()),
                    None,
                )],
                summary: empty_summary(cfg, used_vars),
            };
        }

        if !seen.insert(spec.name.clone()) {
            return EnvRegistryResult {
                violations: vec![mk_violation(
                    "env_registry.registry_invalid",
                    format!("duplicate env var in registry: {}", spec.name),
                    Some(cfg.registry_path.clone()),
                    None,
                )],
                summary: empty_summary(cfg, used_vars),
            };
        }

        specs.push(spec);
    }

    specs.sort_by(|a, b| a.name.cmp(&b.name));

    let registered: HashSet<&str> = specs.iter().map(|v| v.name.as_str()).collect();
    let mut violations: Vec<Violation> = vec![];

    for (env_name, tool_ids) in &usage {
        if !registered.contains(env_name.as_str()) {
            violations.push(mk_violation(
                "env_registry.unregistered_usage",
                format!(
                    "env var {} is used by tools but missing in registry {}",
                    env_name, cfg.registry_path
                ),
                Some(".agents/mcp/compas/plugins".to_string()),
                Some(json!({
                    "var": env_name,
                    "used_by_tools": tool_ids.iter().collect::<Vec<_>>(),
                    "registry_path": cfg.registry_path,
                })),
            ));
        }
    }

    let mut entries: Vec<EffectiveConfigEntry> = vec![];

    for spec in specs {
        let used_by_tools = usage
            .get(&spec.name)
            .map(|s| s.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();

        let (source, value) = if let Ok(v) = std::env::var(&spec.name) {
            (
                EffectiveConfigSource::Env,
                Some(redact_value(v, spec.sensitive)),
            )
        } else if std::env::var_os(&spec.name).is_some() {
            (
                EffectiveConfigSource::Env,
                Some(redact_value("<non-utf8>".to_string(), spec.sensitive)),
            )
        } else if let Some(default) = spec.default.clone() {
            (
                EffectiveConfigSource::Default,
                Some(redact_value(default, spec.sensitive)),
            )
        } else {
            (EffectiveConfigSource::Unset, None)
        };

        if spec.required && matches!(source, EffectiveConfigSource::Unset) {
            violations.push(mk_violation(
                "env_registry.required_missing",
                format!(
                    "required env var {} is missing and has no default",
                    spec.name
                ),
                Some(cfg.registry_path.clone()),
                Some(json!({ "var": spec.name })),
            ));
        }

        entries.push(EffectiveConfigEntry {
            name: spec.name,
            description: spec.description,
            required: spec.required,
            sensitive: spec.sensitive,
            source,
            value,
            used_by_tools,
        });
    }

    EnvRegistryResult {
        violations,
        summary: EffectiveConfigSummary {
            registry_path: cfg.registry_path.clone(),
            registered_vars: entries.len(),
            used_vars,
            entries,
        },
    }
}
