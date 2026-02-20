use crate::config::{ProjectTool, ToolExecutionPolicyConfigV2, ToolExecutionPolicyMode};
use crate::repo::RepoConfigError;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};

const DEFAULT_ALLOWED_COMMANDS: &[&str] = &[
    "cargo",
    "cargo-nextest",
    "python",
    "python3",
    "node",
    "npm",
    "pnpm",
    "yarn",
    "go",
    "dotnet",
    "cmake",
    "ctest",
    "bash",
    "sh",
    "pwsh",
    "powershell",
    "make",
    "just",
    "echo",
    "uv",
    "pytest",
    "ruff",
    "mypy",
    "clang",
    "clang++",
    "gcc",
    "g++",
    "csc",
    "msbuild",
];

pub(crate) fn id_regex() -> Regex {
    Regex::new(r"^[a-z0-9][a-z0-9_-]{1,63}$").expect("valid id regex")
}

pub(crate) fn validate_description(
    kind: &str,
    id: &str,
    description: &str,
) -> Result<(), RepoConfigError> {
    let normalized = description.trim();
    if normalized.is_empty() {
        return Err(RepoConfigError::InvalidDescription {
            kind: kind.to_string(),
            id: id.to_string(),
            message: "description is required".to_string(),
        });
    }
    let len = normalized.chars().count();
    if !(12..=220).contains(&len) {
        return Err(RepoConfigError::InvalidDescription {
            kind: kind.to_string(),
            id: id.to_string(),
            message: "description length must be between 12 and 220 chars".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn validate_tool(plugin_id: &str, tool: &ProjectTool) -> Result<(), RepoConfigError> {
    let id_re = id_regex();
    if !id_re.is_match(&tool.id) {
        return Err(RepoConfigError::InvalidToolId {
            plugin_id: plugin_id.to_string(),
            tool_id: tool.id.clone(),
        });
    }
    validate_description("tool", &tool.id, &tool.description)?;
    if tool.command.trim().is_empty() {
        return Err(RepoConfigError::InvalidToolCommand {
            plugin_id: plugin_id.to_string(),
            tool_id: tool.id.clone(),
        });
    }
    Ok(())
}

fn command_basename(command: &str) -> String {
    command
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn command_like_regex() -> Regex {
    Regex::new(r"^[a-z0-9][a-z0-9._+\-]{0,63}$").expect("valid command-like regex")
}

pub(crate) fn validate_tool_policy(
    plugin_id: &str,
    policy: &ToolExecutionPolicyConfigV2,
) -> Result<(), RepoConfigError> {
    let re = command_like_regex();
    for raw in &policy.allow_commands {
        let cmd = command_basename(raw);
        if cmd.is_empty() || !re.is_match(&cmd) {
            return Err(RepoConfigError::InvalidToolPolicyCommand {
                plugin_id: plugin_id.to_string(),
                command: raw.clone(),
            });
        }
    }
    Ok(())
}

pub(crate) fn enforce_tool_execution_policy(
    plugin_id: &str,
    tool: &ProjectTool,
    policy: &ToolExecutionPolicyConfigV2,
) -> Result<(), RepoConfigError> {
    if matches!(policy.mode, ToolExecutionPolicyMode::AllowAny) {
        return Ok(());
    }

    let mut allowset: BTreeSet<String> = DEFAULT_ALLOWED_COMMANDS
        .iter()
        .map(|v| (*v).to_string())
        .collect();
    for entry in &policy.allow_commands {
        let cmd = command_basename(entry);
        if !cmd.is_empty() {
            allowset.insert(cmd);
        }
    }

    let command = command_basename(&tool.command);
    if allowset.contains(&command) {
        Ok(())
    } else {
        Err(RepoConfigError::ToolCommandPolicyViolation {
            plugin_id: plugin_id.to_string(),
            tool_id: tool.id.clone(),
            command,
            mode: "allowlist".to_string(),
        })
    }
}

pub(crate) fn ensure_known_gate_tools(
    plugin_id: &str,
    gate_kind: &str,
    tool_ids: &[String],
    tools: &BTreeMap<String, ProjectTool>,
) -> Result<(), RepoConfigError> {
    for tool_id in tool_ids {
        if !tools.contains_key(tool_id) {
            return Err(RepoConfigError::UnknownGateTool {
                plugin_id: plugin_id.to_string(),
                gate_kind: gate_kind.to_string(),
                tool_id: tool_id.clone(),
            });
        }
    }
    Ok(())
}
