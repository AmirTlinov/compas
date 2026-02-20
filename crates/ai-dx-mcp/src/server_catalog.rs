use crate::api::*;
use crate::repo::{RepoConfig, load_repo_config};
use crate::repo_view::{
    to_public_plugin_info, to_public_plugin_spec, to_public_tool_spec_with_owner,
};
use crate::runner::run_project_tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatalogView {
    All,
    Plugins,
    Plugin,
    Tools,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogRequest {
    pub(crate) repo_root: Option<String>,
    pub(crate) view: Option<CatalogView>,
    pub(crate) plugin_id: Option<String>,
    pub(crate) tool_id: Option<String>,
    #[serde(default)]
    pub(crate) response_mode: Option<ResponseMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct CatalogOutput {
    pub(crate) ok: bool,
    pub(crate) error: Option<ApiError>,
    pub(crate) repo_root: String,
    pub(crate) plugins: Option<Vec<PluginInfo>>,
    pub(crate) plugin: Option<PluginSpec>,
    pub(crate) tools: Option<Vec<ProjectToolInfo>>,
    pub(crate) tool: Option<ProjectToolSpec>,
    #[serde(default)]
    pub(crate) summary_md: Option<String>,
    #[serde(default)]
    pub(crate) payload_meta: Option<PayloadMeta>,
}

fn missing_tool_owner_error(tool_id: &str) -> ApiError {
    ApiError {
        code: "compas.catalog.tool_owner_missing".to_string(),
        message: format!("internal invariant broken: missing owner for tool_id={tool_id}"),
    }
}

fn tool_owner<'a>(cfg: &'a RepoConfig, tool_id: &str) -> Result<&'a str, ApiError> {
    cfg.tool_owners
        .get(tool_id)
        .map(String::as_str)
        .ok_or_else(|| missing_tool_owner_error(tool_id))
}

fn collect_tools(cfg: &RepoConfig) -> Result<Vec<ProjectToolInfo>, ApiError> {
    let mut tools: Vec<ProjectToolInfo> = cfg
        .tools
        .iter()
        .map(|(tool_id, t)| {
            let owner = tool_owner(cfg, tool_id)?;
            Ok(ProjectToolInfo {
                id: t.id.clone(),
                plugin_id: owner.to_string(),
                description: t.description.clone(),
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    tools.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tools)
}

fn collect_plugins(cfg: &RepoConfig) -> Vec<PluginInfo> {
    let mut plugins: Vec<PluginInfo> = cfg.plugins.values().map(to_public_plugin_info).collect();
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    plugins
}

fn catalog_err(repo_root: &str, error: ApiError) -> CatalogOutput {
    CatalogOutput {
        ok: false,
        error: Some(error),
        repo_root: repo_root.to_string(),
        plugins: None,
        plugin: None,
        tools: None,
        tool: None,
        summary_md: None,
        payload_meta: None,
    }
}

pub(crate) fn catalog(repo_root: &str, req: &CatalogRequest) -> CatalogOutput {
    let view = req.view.unwrap_or(CatalogView::All);
    let cfg = match load_repo_config(std::path::Path::new(repo_root)) {
        Ok(c) => c,
        Err(e) => return catalog_err(repo_root, crate::app::map_config_error(repo_root, e)),
    };

    match view {
        CatalogView::All => match collect_tools(&cfg) {
            Ok(tools) => CatalogOutput {
                ok: true,
                error: None,
                repo_root: repo_root.to_string(),
                plugins: Some(collect_plugins(&cfg)),
                plugin: None,
                tools: Some(tools),
                tool: None,
                summary_md: None,
                payload_meta: None,
            },
            Err(err) => catalog_err(repo_root, err),
        },
        CatalogView::Plugins => CatalogOutput {
            ok: true,
            error: None,
            repo_root: repo_root.to_string(),
            plugins: Some(collect_plugins(&cfg)),
            plugin: None,
            tools: None,
            tool: None,
            summary_md: None,
            payload_meta: None,
        },
        CatalogView::Tools => match collect_tools(&cfg) {
            Ok(tools) => CatalogOutput {
                ok: true,
                error: None,
                repo_root: repo_root.to_string(),
                plugins: None,
                plugin: None,
                tools: Some(tools),
                tool: None,
                summary_md: None,
                payload_meta: None,
            },
            Err(err) => catalog_err(repo_root, err),
        },
        CatalogView::Plugin => {
            let plugin_id = match req.plugin_id.as_ref() {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return catalog_err(
                        repo_root,
                        ApiError {
                            code: "compas.catalog.plugin_id_required".to_string(),
                            message: "view=plugin requires plugin_id".to_string(),
                        },
                    );
                }
            };
            let plugin = cfg
                .plugins
                .get(plugin_id)
                .map(|p| to_public_plugin_spec(p, &cfg.tools));
            if plugin.is_none() {
                return catalog_err(
                    repo_root,
                    ApiError {
                        code: "compas.catalog.unknown_plugin_id".to_string(),
                        message: format!(
                            "unknown plugin_id={plugin_id}; run compas.catalog with view=plugins"
                        ),
                    },
                );
            }
            CatalogOutput {
                ok: true,
                error: None,
                repo_root: repo_root.to_string(),
                plugins: None,
                plugin,
                tools: None,
                tool: None,
                summary_md: None,
                payload_meta: None,
            }
        }
        CatalogView::Tool => {
            let tool_id = match req.tool_id.as_ref() {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return catalog_err(
                        repo_root,
                        ApiError {
                            code: "compas.catalog.tool_id_required".to_string(),
                            message: "view=tool requires tool_id".to_string(),
                        },
                    );
                }
            };
            let tool = match cfg.tools.get(tool_id) {
                Some(tool) => {
                    let owner = match tool_owner(&cfg, tool_id) {
                        Ok(o) => o,
                        Err(err) => return catalog_err(repo_root, err),
                    };
                    Some(to_public_tool_spec_with_owner(tool, owner))
                }
                None => None,
            };
            if tool.is_none() {
                return catalog_err(
                    repo_root,
                    ApiError {
                        code: "compas.catalog.unknown_tool_id".to_string(),
                        message: format!(
                            "unknown tool_id={tool_id}; run compas.catalog with view=tools"
                        ),
                    },
                );
            }
            CatalogOutput {
                ok: true,
                error: None,
                repo_root: repo_root.to_string(),
                plugins: None,
                plugin: None,
                tools: None,
                tool,
                summary_md: None,
                payload_meta: None,
            }
        }
    }
}

pub(crate) async fn exec(repo_root: &str, req: &ToolsRunRequest) -> ToolsRunOutput {
    let dry_run = req.dry_run.unwrap_or(false);
    let extra_args = req.args.clone().unwrap_or_default();

    let cfg = match load_repo_config(std::path::Path::new(repo_root)) {
        Ok(c) => c,
        Err(e) => {
            return ToolsRunOutput {
                ok: false,
                error: Some(crate::app::map_config_error(repo_root, e)),
                repo_root: repo_root.to_string(),
                receipt: None,
                summary_md: None,
                payload_meta: None,
            };
        }
    };

    let tool = match cfg.tools.get(&req.tool_id) {
        Some(t) => t,
        None => {
            return ToolsRunOutput {
                ok: false,
                error: Some(ApiError {
                    code: "compas.exec.unknown_tool_id".to_string(),
                    message: format!(
                        "unknown tool_id={}; run compas.catalog with view=tools",
                        req.tool_id
                    ),
                }),
                repo_root: repo_root.to_string(),
                receipt: None,
                summary_md: None,
                payload_meta: None,
            };
        }
    };

    match run_project_tool(std::path::Path::new(repo_root), tool, &extra_args, dry_run).await {
        Ok(receipt) => {
            let error = if receipt.success {
                None
            } else {
                Some(ApiError {
                    code: "compas.exec.exit_nonzero".to_string(),
                    message: format!(
                        "tool failed: tool_id={}; exit_code={:?}; timed_out={}",
                        receipt.tool_id, receipt.exit_code, receipt.timed_out
                    ),
                })
            };

            ToolsRunOutput {
                ok: receipt.success,
                error,
                repo_root: repo_root.to_string(),
                receipt: Some(receipt),
                summary_md: None,
                payload_meta: None,
            }
        }
        Err(e) => ToolsRunOutput {
            ok: false,
            error: Some(ApiError {
                code: "compas.exec.run_failed".to_string(),
                message: e.to_string(),
            }),
            repo_root: repo_root.to_string(),
            receipt: None,
            summary_md: None,
            payload_meta: None,
        },
    }
}
