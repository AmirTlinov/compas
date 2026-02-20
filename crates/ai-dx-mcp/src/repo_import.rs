use crate::config::ProjectTool;
use crate::repo::RepoConfigError;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedToolFile {
    tool: ProjectTool,
}

pub(crate) fn load_imported_tools(
    repo_root: &Path,
    plugin_id: &str,
    pattern: &str,
) -> Result<Vec<ProjectTool>, RepoConfigError> {
    let abs_pattern = repo_root.join(pattern).to_string_lossy().into_owned();
    let entries = glob::glob(&abs_pattern).map_err(|e| RepoConfigError::InvalidImportGlob {
        plugin_id: plugin_id.to_string(),
        pattern: pattern.to_string(),
        message: e.msg.to_string(),
    })?;

    let mut paths: Vec<PathBuf> =
        entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| RepoConfigError::ReadImportedTool {
                path: e.path().to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            })?;
    paths.sort();

    let mut tools = Vec::with_capacity(paths.len());
    for path in paths {
        let raw = fs::read_to_string(&path).map_err(|e| RepoConfigError::ReadImportedTool {
            path: path.clone(),
            source: e,
        })?;
        let imported: ImportedToolFile =
            toml::from_str(&raw).map_err(|e| RepoConfigError::ParseImportedTool {
                path: path.clone(),
                message: e.to_string(),
            })?;

        tools.push(imported.tool);
    }
    Ok(tools)
}
