use super::planner::detected_repo_languages;
use crate::{
    api::{ApiError, InitRecommendations, InitRegistryPackRecommendation, InitRequest},
    registry_manifest::{
        RegistryManifestV1, RegistryPackRecommendationV1, RegistryPackV1, RegistryPluginV1,
    },
};
use std::{cmp::Reverse, path::Path};

fn api_err(code: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        code: code.to_string(),
        message: message.into(),
    }
}

fn matches_recommendation(rec: &RegistryPackRecommendationV1, languages: &[String]) -> bool {
    if rec.when_no_languages {
        return languages.is_empty();
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
    true
}

pub(crate) fn recommendations_for_manifest_languages(
    manifest: &RegistryManifestV1,
    languages: &[String],
) -> Vec<InitRegistryPackRecommendation> {
    let mut candidates: Vec<(u32, String, InitRegistryPackRecommendation)> = manifest
        .packs
        .iter()
        .filter_map(|pack| pack_recommendation(manifest, pack, languages))
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
) -> Option<(u32, String, InitRegistryPackRecommendation)> {
    let recommendation = pack.recommendation.as_ref()?;
    if !matches_recommendation(recommendation, languages) {
        return None;
    }
    let (requires, runtime_kind, cost_class) = effective_pack_metadata(manifest, pack);
    Some((
        recommendation.priority,
        pack.id.clone(),
        InitRegistryPackRecommendation {
            pack_id: pack.id.clone(),
            why: recommendation.why.clone(),
            cost_class,
            runtime_kind,
            requires,
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
    let recommended = recommendations_for_manifest_languages(&resolved.manifest, &languages);
    if recommended.is_empty() {
        return Ok(None);
    }
    Ok(Some(InitRecommendations { recommended }))
}

#[cfg(test)]
mod tests;
