use crate::config::{
    ChecksConfigV2, GateConfig, PluginConfig, ProjectTool, QualityContractConfig,
    ToolExecutionPolicyMode,
};
use crate::repo_import::load_imported_tools;
use crate::repo_strict::{
    enforce_tool_execution_policy, ensure_known_gate_tools, id_regex, validate_description,
    validate_tool, validate_tool_policy,
};
use crate::repo_view::RepoPlugin;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

mod checks_merge;
mod errors;
use checks_merge::push_check_with_unique_id;
pub use errors::RepoConfigError;

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub tools: BTreeMap<String, ProjectTool>,
    pub(crate) tool_owners: BTreeMap<String, String>,
    pub(crate) plugins: BTreeMap<String, RepoPlugin>,
    pub gate: GateConfig,
    pub checks: ChecksConfigV2,
    pub quality_contract: Option<QualityContractConfig>,
    pub allow_any_plugins: Vec<String>,
}

pub fn load_repo_config(repo_root: &Path) -> Result<RepoConfig, RepoConfigError> {
    let plugins_dir = repo_root.join(".agents/mcp/compas/plugins");
    if !plugins_dir.is_dir() {
        return Err(RepoConfigError::PluginsDirMissing(plugins_dir));
    }

    let mut plugin_tomls: Vec<PathBuf> = vec![];
    let read_dir = fs::read_dir(&plugins_dir).map_err(|e| RepoConfigError::ReadPlugin {
        path: plugins_dir.clone(),
        source: e,
    })?;
    for entry in read_dir {
        let entry = entry.map_err(|e| RepoConfigError::ReadPlugin {
            path: plugins_dir.clone(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let plugin_toml = path.join("plugin.toml");
        if plugin_toml.is_file() {
            plugin_tomls.push(plugin_toml);
        }
    }
    plugin_tomls.sort();

    let mut tools: BTreeMap<String, ProjectTool> = BTreeMap::new();
    let mut plugins: BTreeMap<String, RepoPlugin> = BTreeMap::new();
    let mut tool_owners: BTreeMap<String, String> = BTreeMap::new();
    let mut allow_any_plugins: Vec<String> = vec![];
    let mut gate: GateConfig = GateConfig {
        ci_fast: vec![],
        ci: vec![],
        flagship: vec![],
    };
    let mut checks: ChecksConfigV2 = ChecksConfigV2 {
        loc: vec![],
        env_registry: vec![],
        boundary: vec![],
        surface: vec![],
        duplicates: vec![],
        supply_chain: vec![],
        tool_budget: vec![],
        reuse_first: vec![],
        arch_layers: vec![],
        dead_code: vec![],
        orphan_api: vec![],
        complexity_budget: vec![],
        contract_break: vec![],
    };

    let mut any_config = false;
    let id_re = id_regex();
    let mut loc_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut env_registry_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut boundary_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut surface_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut supply_chain_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut tool_budget_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut reuse_first_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut arch_layers_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut dead_code_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut orphan_api_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut complexity_budget_check_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut contract_break_check_ids: BTreeMap<String, String> = BTreeMap::new();

    for path in plugin_tomls {
        any_config = true;
        let raw = fs::read_to_string(&path).map_err(|e| RepoConfigError::ReadPlugin {
            path: path.clone(),
            source: e,
        })?;
        let plugin: PluginConfig =
            toml::from_str(&raw).map_err(|e| RepoConfigError::ParsePlugin {
                path: path.clone(),
                message: e.to_string(),
            })?;
        let plugin_id = plugin.plugin.id.clone();
        if !id_re.is_match(&plugin_id) {
            return Err(RepoConfigError::InvalidPluginId {
                plugin_id: plugin_id.clone(),
            });
        }
        validate_description("plugin", &plugin_id, &plugin.plugin.description)?;
        if plugins.contains_key(&plugin_id) {
            return Err(RepoConfigError::DuplicatePluginId { plugin_id });
        }
        validate_tool_policy(&plugin_id, &plugin.tool_policy)?;
        let tool_policy = plugin.tool_policy.clone();
        if matches!(tool_policy.mode, ToolExecutionPolicyMode::AllowAny) {
            allow_any_plugins.push(plugin_id.clone());
        }

        let mut plugin_tool_ids: Vec<String> = vec![];

        for tool in plugin.tools {
            validate_tool(&plugin_id, &tool)?;
            enforce_tool_execution_policy(&plugin_id, &tool, &tool_policy)?;
            let tool_id = tool.id.clone();
            if tools.contains_key(&tool_id) {
                return Err(RepoConfigError::DuplicateTool {
                    tool_id,
                    plugin_id: plugin_id.clone(),
                });
            }
            plugin_tool_ids.push(tool.id.clone());
            tool_owners.insert(tool.id.clone(), plugin_id.clone());
            tools.insert(tool.id.clone(), tool);
        }

        for pattern in &plugin.plugin.tool_import_globs {
            for tool in load_imported_tools(repo_root, &plugin_id, pattern)? {
                validate_tool(&plugin_id, &tool)?;
                enforce_tool_execution_policy(&plugin_id, &tool, &tool_policy)?;
                let tool_id = tool.id.clone();
                if tools.contains_key(&tool_id) {
                    return Err(RepoConfigError::DuplicateTool {
                        tool_id,
                        plugin_id: plugin_id.clone(),
                    });
                }
                plugin_tool_ids.push(tool.id.clone());
                tool_owners.insert(tool.id.clone(), plugin_id.clone());
                tools.insert(tool.id.clone(), tool);
            }
        }

        let gate_cfg = plugin.gate.unwrap_or(GateConfig {
            ci_fast: vec![],
            ci: vec![],
            flagship: vec![],
        });
        if !(gate_cfg.ci_fast.is_empty() && gate_cfg.ci.is_empty() && gate_cfg.flagship.is_empty())
        {
            // Merge strategy: append in plugin order (deterministic by path sorting).
            gate.ci_fast.extend(gate_cfg.ci_fast.clone());
            gate.ci.extend(gate_cfg.ci.clone());
            gate.flagship.extend(gate_cfg.flagship.clone());
        }

        let checks_cfg = plugin.checks;
        let has_any_check = checks_cfg.as_ref().is_some_and(|c| {
            !(c.loc.is_empty()
                && c.env_registry.is_empty()
                && c.boundary.is_empty()
                && c.surface.is_empty()
                && c.duplicates.is_empty()
                && c.supply_chain.is_empty()
                && c.tool_budget.is_empty()
                && c.reuse_first.is_empty()
                && c.arch_layers.is_empty()
                && c.dead_code.is_empty()
                && c.orphan_api.is_empty()
                && c.complexity_budget.is_empty()
                && c.contract_break.is_empty())
        });
        if let Some(c) = checks_cfg {
            // Merge strategy: append in plugin order (deterministic by path sorting).
            for v in c.loc {
                push_check_with_unique_id(
                    &mut checks.loc,
                    v,
                    "loc",
                    &plugin_id,
                    &id_re,
                    &mut loc_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.env_registry {
                push_check_with_unique_id(
                    &mut checks.env_registry,
                    v,
                    "env_registry",
                    &plugin_id,
                    &id_re,
                    &mut env_registry_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.boundary {
                push_check_with_unique_id(
                    &mut checks.boundary,
                    v,
                    "boundary",
                    &plugin_id,
                    &id_re,
                    &mut boundary_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.surface {
                push_check_with_unique_id(
                    &mut checks.surface,
                    v,
                    "surface",
                    &plugin_id,
                    &id_re,
                    &mut surface_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.duplicates {
                push_check_with_unique_id(
                    &mut checks.duplicates,
                    v,
                    "duplicates",
                    &plugin_id,
                    &id_re,
                    &mut duplicates_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.supply_chain {
                push_check_with_unique_id(
                    &mut checks.supply_chain,
                    v,
                    "supply_chain",
                    &plugin_id,
                    &id_re,
                    &mut supply_chain_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.tool_budget {
                push_check_with_unique_id(
                    &mut checks.tool_budget,
                    v,
                    "tool_budget",
                    &plugin_id,
                    &id_re,
                    &mut tool_budget_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.reuse_first {
                push_check_with_unique_id(
                    &mut checks.reuse_first,
                    v,
                    "reuse_first",
                    &plugin_id,
                    &id_re,
                    &mut reuse_first_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.arch_layers {
                push_check_with_unique_id(
                    &mut checks.arch_layers,
                    v,
                    "arch_layers",
                    &plugin_id,
                    &id_re,
                    &mut arch_layers_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.dead_code {
                push_check_with_unique_id(
                    &mut checks.dead_code,
                    v,
                    "dead_code",
                    &plugin_id,
                    &id_re,
                    &mut dead_code_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.orphan_api {
                push_check_with_unique_id(
                    &mut checks.orphan_api,
                    v,
                    "orphan_api",
                    &plugin_id,
                    &id_re,
                    &mut orphan_api_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.complexity_budget {
                push_check_with_unique_id(
                    &mut checks.complexity_budget,
                    v,
                    "complexity_budget",
                    &plugin_id,
                    &id_re,
                    &mut complexity_budget_check_ids,
                    |x| &x.id,
                )?;
            }
            for v in c.contract_break {
                push_check_with_unique_id(
                    &mut checks.contract_break,
                    v,
                    "contract_break",
                    &plugin_id,
                    &id_re,
                    &mut contract_break_check_ids,
                    |x| &x.id,
                )?;
            }
        }
        let has_gate = !(gate_cfg.ci_fast.is_empty()
            && gate_cfg.ci.is_empty()
            && gate_cfg.flagship.is_empty());
        let has_tools = !plugin_tool_ids.is_empty();
        if !(has_any_check || has_gate || has_tools) {
            return Err(RepoConfigError::EmptyPlugin {
                plugin_id: plugin_id.clone(),
            });
        }

        plugin_tool_ids.sort();
        plugins.insert(
            plugin_id.clone(),
            RepoPlugin {
                id: plugin_id,
                description: plugin.plugin.description,
                tool_ids: plugin_tool_ids,
                gate_ci_fast: gate_cfg.ci_fast,
                gate_ci: gate_cfg.ci,
                gate_flagship: gate_cfg.flagship,
            },
        );
    }

    if !any_config {
        return Err(RepoConfigError::EmptyConfig);
    }

    allow_any_plugins.sort();

    let quality_contract_path = repo_root.join(".agents/mcp/compas/quality_contract.toml");
    let quality_contract = if quality_contract_path.is_file() {
        let raw = fs::read_to_string(&quality_contract_path).map_err(|e| {
            RepoConfigError::ReadQualityContract {
                path: quality_contract_path.clone(),
                source: e,
            }
        })?;
        let parsed = toml::from_str::<QualityContractConfig>(&raw).map_err(|e| {
            RepoConfigError::ParseQualityContract {
                path: quality_contract_path.clone(),
                message: e.to_string(),
            }
        })?;
        Some(parsed)
    } else {
        None
    };

    for plugin in plugins.values() {
        ensure_known_gate_tools(&plugin.id, "ci_fast", &plugin.gate_ci_fast, &tools)?;
        ensure_known_gate_tools(&plugin.id, "ci", &plugin.gate_ci, &tools)?;
        ensure_known_gate_tools(&plugin.id, "flagship", &plugin.gate_flagship, &tools)?;
    }
    for tool_id in tools.keys() {
        if !tool_owners.contains_key(tool_id) {
            return Err(RepoConfigError::MissingToolOwner {
                tool_id: tool_id.clone(),
            });
        }
    }

    Ok(RepoConfig {
        tools,
        tool_owners,
        plugins,
        gate,
        checks,
        quality_contract,
        allow_any_plugins,
    })
}
