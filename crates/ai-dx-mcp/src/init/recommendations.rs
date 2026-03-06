use super::planner::detected_repo_languages;
use crate::{
    api::{ApiError, InitRecommendations, InitRegistryPackRecommendation, InitRequest},
    registry_manifest::{
        RegistryManifestV1, RegistryPackRecommendationV1, RegistryPackV1, RegistryPluginV1,
    },
};
use std::{cmp::Reverse, collections::BTreeSet, fs, path::Path};

fn api_err(code: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        code: code.to_string(),
        message: message.into(),
    }
}

const AI_FIRST_REQUIRED_FILES: &[&str] = &[
    "AGENTS.md",
    "ARCHITECTURE.md",
    "docs/index.md",
    "docs/exec-plans/README.md",
    "docs/exec-plans/TEMPLATE.md",
    "docs/QUALITY_SCORE.md",
];
const AI_FIRST_AGENTS_MARKER: &str = "<!-- compas:ai_first_router -->";
const QUALITY_SCORE_START_MARKER: &str = "<!-- compas:quality_score:start -->";
const QUALITY_SCORE_END_MARKER: &str = "<!-- compas:quality_score:end -->";

fn detect_repo_signals(repo_root: &Path) -> Vec<String> {
    let mut signals: BTreeSet<String> = BTreeSet::new();
    if has_ai_first_scaffold_signal(repo_root) {
        signals.insert("ai_first_scaffold".to_string());
    }
    for (signal, rel) in [
        (
            "worktree_isolation_declared",
            ".agents/mcp/compas/runtime/worktree_isolation.toml",
        ),
        (
            "app_harness_declared",
            ".agents/mcp/compas/runtime/app_harness.toml",
        ),
        (
            "observability_declared",
            ".agents/mcp/compas/runtime/observability.toml",
        ),
        (
            "ui_validation_declared",
            ".agents/mcp/compas/runtime/ui_validation.toml",
        ),
    ] {
        if repo_root.join(rel).is_file() {
            signals.insert(signal.to_string());
        }
    }
    signals.into_iter().collect()
}

fn has_ai_first_scaffold_signal(repo_root: &Path) -> bool {
    if !AI_FIRST_REQUIRED_FILES
        .iter()
        .all(|rel| repo_root.join(rel).is_file())
    {
        return false;
    }
    let agents = fs::read_to_string(repo_root.join("AGENTS.md")).ok();
    let quality = fs::read_to_string(repo_root.join("docs/QUALITY_SCORE.md")).ok();
    matches!(agents.as_deref(), Some(text) if text.contains(AI_FIRST_AGENTS_MARKER))
        && matches!(quality.as_deref(), Some(text) if text.contains(QUALITY_SCORE_START_MARKER) && text.contains(QUALITY_SCORE_END_MARKER))
}

fn matches_recommendation(
    rec: &RegistryPackRecommendationV1,
    languages: &[String],
    repo_signals: &[String],
) -> bool {
    if rec.when_no_languages {
        if !languages.is_empty() {
            return false;
        }
    }
    if !rec.languages_any.is_empty()
        && !languages.iter().any(|language| {
            rec.languages_any
                .iter()
                .any(|candidate| candidate == language)
        })
    {
        return false;
    }
    if !rec.languages_all.is_empty()
        && !rec
            .languages_all
            .iter()
            .all(|required| languages.iter().any(|language| language == required))
        {
        return false;
    }
    if !rec.signals_any.is_empty()
        && !repo_signals
            .iter()
            .any(|signal| rec.signals_any.iter().any(|candidate| candidate == signal))
    {
        return false;
    }
    if !rec.signals_all.is_empty()
        && !rec
            .signals_all
            .iter()
            .all(|required| repo_signals.iter().any(|signal| signal == required))
    {
        return false;
    }
    true
}

pub(crate) fn recommendations_for_manifest_languages(
    manifest: &RegistryManifestV1,
    languages: &[String],
    repo_signals: &[String],
) -> Vec<InitRegistryPackRecommendation> {
    let mut candidates: Vec<(u32, String, InitRegistryPackRecommendation)> = manifest
        .packs
        .iter()
        .filter_map(|pack| pack_recommendation(manifest, pack, languages, repo_signals))
        .collect();
    candidates.sort_by_key(|(priority, pack_id, _)| (Reverse(*priority), pack_id.clone()));
    candidates
        .into_iter()
        .map(|(_, _, recommendation)| recommendation)
        .collect()
}

fn pack_recommendation(
    manifest: &RegistryManifestV1,
    pack: &RegistryPackV1,
    languages: &[String],
    repo_signals: &[String],
) -> Option<(u32, String, InitRegistryPackRecommendation)> {
    let recommendation = pack.recommendation.as_ref()?;
    if !matches_recommendation(recommendation, languages, repo_signals) {
        return None;
    }
    let (requires, runtime_kind, cost_class) = effective_pack_metadata(manifest, pack);
    let mut matched_signals = recommendation
        .signals_all
        .iter()
        .filter(|signal| repo_signals.iter().any(|present| present == *signal))
        .cloned()
        .collect::<Vec<_>>();
    matched_signals.extend(
        recommendation
            .signals_any
            .iter()
            .filter(|signal| repo_signals.iter().any(|present| present == *signal))
            .cloned(),
    );
    matched_signals.sort();
    matched_signals.dedup();
    Some((
        recommendation.priority,
        pack.id.clone(),
        InitRegistryPackRecommendation {
            pack_id: pack.id.clone(),
            why: recommendation.why.clone(),
            cost_class,
            runtime_kind,
            requires,
            matched_signals,
        },
    ))
}

fn effective_pack_metadata(
    manifest: &RegistryManifestV1,
    pack: &RegistryPackV1,
) -> (Vec<String>, String, String) {
    if !pack.runtime_kind.trim().is_empty() && !pack.cost_class.trim().is_empty() {
        return (
            pack.requires.clone(),
            pack.runtime_kind.clone(),
            pack.cost_class.clone(),
        );
    }

    let members: Vec<&RegistryPluginV1> = pack
        .plugins
        .iter()
        .filter_map(|plugin_id| {
            manifest
                .plugins
                .iter()
                .find(|plugin| &plugin.id == plugin_id)
        })
        .collect();

    let mut requires = members
        .iter()
        .flat_map(|plugin| {
            plugin
                .extra
                .get("requires")
                .and_then(|value| value.as_array())
                .into_iter()
                .flat_map(|items| items.iter())
                .filter_map(|value| value.as_str())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    requires.sort();
    requires.dedup();

    let runtime_kinds = members
        .iter()
        .filter_map(|plugin| {
            plugin
                .extra
                .get("runtime_kind")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    let runtime_kind = match runtime_kinds.first() {
        None => pack.runtime_kind.clone(),
        Some(first) if runtime_kinds.iter().all(|value| value == first) => first.clone(),
        Some(_) => "mixed".to_string(),
    };

    let cost_rank = |value: &str| match value {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    };
    let cost_class = members
        .iter()
        .filter_map(|plugin| {
            plugin
                .extra
                .get("cost_class")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
        })
        .max_by_key(|value| cost_rank(value))
        .map(ToString::to_string)
        .unwrap_or_else(|| pack.cost_class.clone());

    (requires, runtime_kind, cost_class)
}

pub(crate) async fn registry_pack_recommendations(
    repo_root: &Path,
    req: &InitRequest,
) -> Result<Option<InitRecommendations>, ApiError> {
    let Some(source) = req
        .registry_source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let resolved = crate::registry_manifest::load_verified_manifest_source(source, false, None)
        .await
        .map_err(|e| {
            api_err(
                "init.registry_manifest_load_failed",
                format!("failed to load verified registry manifest from {source}: {e}"),
            )
        })?;
    let languages = detected_repo_languages(repo_root, req)?;
    let repo_signals = detect_repo_signals(repo_root);
    let recommended =
        recommendations_for_manifest_languages(&resolved.manifest, &languages, &repo_signals);
    if recommended.is_empty() && repo_signals.is_empty() {
        return Ok(None);
    }
    Ok(Some(InitRecommendations {
        repo_signals,
        recommended,
    }))
}

#[cfg(test)]
mod tests;
