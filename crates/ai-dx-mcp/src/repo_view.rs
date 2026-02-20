use crate::api::{PluginInfo, PluginSpec, ProjectToolSpec};
use crate::config::ProjectTool;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(crate) struct RepoPlugin {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) tool_ids: Vec<String>,
    pub(crate) gate_ci_fast: Vec<String>,
    pub(crate) gate_ci: Vec<String>,
    pub(crate) gate_flagship: Vec<String>,
}

pub(crate) fn to_public_tool_spec_with_owner(
    tool: &ProjectTool,
    plugin_id: &str,
) -> ProjectToolSpec {
    ProjectToolSpec {
        id: tool.id.clone(),
        plugin_id: plugin_id.to_string(),
        description: tool.description.clone(),
        command: tool.command.clone(),
        args: tool.args.clone(),
        cwd: tool.cwd.clone(),
        timeout_ms: tool.timeout_ms.unwrap_or(600_000),
        max_stdout_bytes: tool.max_stdout_bytes.unwrap_or(20_000),
        max_stderr_bytes: tool.max_stderr_bytes.unwrap_or(20_000),
    }
}

pub(crate) fn to_public_plugin_info(plugin: &RepoPlugin) -> PluginInfo {
    PluginInfo {
        id: plugin.id.clone(),
        description: plugin.description.clone(),
        tools: plugin.tool_ids.clone(),
    }
}

pub(crate) fn to_public_plugin_spec(
    plugin: &RepoPlugin,
    tools: &BTreeMap<String, ProjectTool>,
) -> PluginSpec {
    let tool_specs: Vec<ProjectToolSpec> = plugin
        .tool_ids
        .iter()
        .filter_map(|tool_id| {
            tools
                .get(tool_id)
                .map(|tool| to_public_tool_spec_with_owner(tool, &plugin.id))
        })
        .collect();
    PluginSpec {
        id: plugin.id.clone(),
        description: plugin.description.clone(),
        tools: tool_specs,
        gate_ci_fast: plugin.gate_ci_fast.clone(),
        gate_ci: plugin.gate_ci.clone(),
        gate_flagship: plugin.gate_flagship.clone(),
    }
}
