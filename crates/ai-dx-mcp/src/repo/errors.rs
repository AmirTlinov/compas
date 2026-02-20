use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RepoConfigError {
    #[error(
        "compas plugins directory not found: {0} (expected .agents/mcp/compas/plugins/*/plugin.toml; fix: run compas.init (MCP) / `init` (CLI), or add plugin.toml + tool.toml)"
    )]
    PluginsDirMissing(PathBuf),
    #[error("failed to read plugin config: {path}: {source}")]
    ReadPlugin {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse plugin config TOML: {path}: {message}")]
    ParsePlugin { path: PathBuf, message: String },
    #[error("failed to read quality contract TOML: {path}: {source}")]
    ReadQualityContract {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse quality contract TOML: {path}: {message}")]
    ParseQualityContract { path: PathBuf, message: String },
    #[error("invalid tool import glob (plugin {plugin_id}): {pattern}: {message}")]
    InvalidImportGlob {
        plugin_id: String,
        pattern: String,
        message: String,
    },
    #[error("failed to read imported tool config: {path}: {source}")]
    ReadImportedTool {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse imported tool TOML: {path}: {message}")]
    ParseImportedTool { path: PathBuf, message: String },
    #[error("invalid plugin id: {plugin_id}")]
    InvalidPluginId { plugin_id: String },
    #[error("invalid tool id: {tool_id} (plugin {plugin_id})")]
    InvalidToolId { plugin_id: String, tool_id: String },
    #[error("invalid check id: {check_id} (kind {kind}, plugin {plugin_id})")]
    InvalidCheckId {
        plugin_id: String,
        kind: String,
        check_id: String,
    },
    #[error("duplicate plugin id: {plugin_id}")]
    DuplicatePluginId { plugin_id: String },
    #[error("duplicate tool id: {tool_id} (plugin {plugin_id})")]
    DuplicateTool { tool_id: String, plugin_id: String },
    #[error(
        "duplicate check id: {check_id} (kind {kind}) found in plugin {plugin_id}; already defined in plugin {previous_plugin_id}"
    )]
    DuplicateCheckId {
        kind: String,
        check_id: String,
        plugin_id: String,
        previous_plugin_id: String,
    },
    #[error("invalid {kind} description ({id}): {message}")]
    InvalidDescription {
        kind: String,
        id: String,
        message: String,
    },
    #[error("invalid tool command: {tool_id} (plugin {plugin_id})")]
    InvalidToolCommand { plugin_id: String, tool_id: String },
    #[error(
        "tool command not allowed by policy: command={command} tool={tool_id} plugin={plugin_id} mode={mode} (fix: set [tool_policy].mode='allow_any' or add command to [tool_policy].allow_commands)"
    )]
    ToolCommandPolicyViolation {
        plugin_id: String,
        tool_id: String,
        command: String,
        mode: String,
    },
    #[error(
        "invalid [tool_policy].allow_commands entry: {command} (plugin {plugin_id}); must be non-empty and command-like"
    )]
    InvalidToolPolicyCommand { plugin_id: String, command: String },
    #[error("plugin has no effective config payload: {plugin_id}")]
    EmptyPlugin { plugin_id: String },
    #[error("unknown gate tool reference: {tool_id} in {gate_kind} (plugin {plugin_id})")]
    UnknownGateTool {
        plugin_id: String,
        gate_kind: String,
        tool_id: String,
    },
    #[error("missing tool owner mapping for tool: {tool_id}")]
    MissingToolOwner { tool_id: String },
    #[error(
        "no tools/checks configured (expected at least one plugin.toml under .agents/mcp/compas/plugins/*/; fix: run compas.init (MCP) / `init` (CLI), or add plugin.toml + tool.toml)"
    )]
    EmptyConfig,
}

impl RepoConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            RepoConfigError::PluginsDirMissing(_) => "config.plugins_dir_missing",
            RepoConfigError::ReadPlugin { .. } => "config.read_failed",
            RepoConfigError::ParsePlugin { .. } => "config.parse_failed",
            RepoConfigError::ReadQualityContract { .. } => "config.quality_contract_read_failed",
            RepoConfigError::ParseQualityContract { .. } => "config.quality_contract_parse_failed",
            RepoConfigError::InvalidImportGlob { .. } => "config.import_glob_invalid",
            RepoConfigError::ReadImportedTool { .. } => "config.import_read_failed",
            RepoConfigError::ParseImportedTool { .. } => "config.import_parse_failed",
            RepoConfigError::InvalidPluginId { .. } => "config.invalid_plugin_id",
            RepoConfigError::InvalidToolId { .. } => "config.invalid_tool_id",
            RepoConfigError::InvalidCheckId { .. } => "config.invalid_check_id",
            RepoConfigError::DuplicatePluginId { .. } => "config.duplicate_plugin_id",
            RepoConfigError::DuplicateTool { .. } => "config.duplicate_tool_id",
            RepoConfigError::DuplicateCheckId { .. } => "config.duplicate_check_id",
            RepoConfigError::InvalidDescription { .. } => "config.invalid_description",
            RepoConfigError::InvalidToolCommand { .. } => "config.invalid_tool_command",
            RepoConfigError::ToolCommandPolicyViolation { .. } => {
                "config.tool_command_policy_violation"
            }
            RepoConfigError::InvalidToolPolicyCommand { .. } => {
                "config.invalid_tool_policy_command"
            }
            RepoConfigError::EmptyPlugin { .. } => "config.empty_plugin",
            RepoConfigError::UnknownGateTool { .. } => "config.unknown_gate_tool",
            RepoConfigError::MissingToolOwner { .. } => "config.missing_tool_owner",
            RepoConfigError::EmptyConfig => "config.empty",
        }
    }
}
