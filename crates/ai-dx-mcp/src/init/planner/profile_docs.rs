use crate::api::{ApiError, InitWriteFile};

use super::api_err;

pub(super) fn quality_contract_toml() -> String {
    include_str!("../templates/quality_contract.toml").to_string()
}

pub(super) fn profile_docs_writes(profile: Option<&str>) -> Result<Vec<InitWriteFile>, ApiError> {
    Ok(match profile {
        None => vec![],
        Some("ai_first") => [
            ("AGENTS.md", include_str!("../templates/ai_first/AGENTS.md")),
            (
                "ARCHITECTURE.md",
                include_str!("../templates/ai_first/ARCHITECTURE.md"),
            ),
            (
                "docs/index.md",
                include_str!("../templates/ai_first/docs/index.md"),
            ),
            (
                "docs/exec-plans/README.md",
                include_str!("../templates/ai_first/docs/exec-plans/README.md"),
            ),
            (
                "docs/QUALITY_SCORE.md",
                include_str!("../templates/ai_first/docs/QUALITY_SCORE.md"),
            ),
        ]
        .into_iter()
        .map(|(path, content_utf8)| InitWriteFile {
            path: path.to_string(),
            content_utf8: content_utf8.to_string(),
        })
        .collect(),
        Some(other) => {
            return Err(api_err(
                "init.unknown_profile",
                format!("unknown init profile: {other}"),
            ));
        }
    })
}
