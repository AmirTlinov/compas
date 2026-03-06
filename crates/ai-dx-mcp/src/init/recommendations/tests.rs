use super::{detect_repo_signals, recommendations_for_manifest_languages};
use crate::{
    api::InitRequest,
    init::planner::plan_init,
    registry_manifest::{
        RegistryArchiveV1, RegistryManifestV1, RegistryPackRecommendationV1, RegistryPackV1,
        RegistryPluginPackageV1, RegistryPluginV1, validate_manifest_v1,
    },
};
use serde_json::json;
use std::collections::BTreeMap;

fn plugin(id: &str) -> RegistryPluginV1 {
    let mut extra = BTreeMap::new();
    extra.insert("capabilities".to_string(), json!(["guard", "lint"]));
    extra.insert("requires".to_string(), json!([]));
    extra.insert("runtime_kind".to_string(), json!("tool-backed"));
    extra.insert("cost_class".to_string(), json!("medium"));
    extra.insert("artifacts_produced".to_string(), json!([]));
    RegistryPluginV1 {
        id: id.to_string(),
        aliases: vec![],
        path: format!("plugins/{id}"),
        status: "community".to_string(),
        owner: "community".to_string(),
        description: format!("Plugin description for {id}"),
        package: RegistryPluginPackageV1 {
            version: "0.1.0".to_string(),
            kind: "tool-backed".to_string(),
            maturity: "stable".to_string(),
            runtime: "native".to_string(),
            portable: true,
            languages: vec!["rust".to_string()],
            entrypoint: "entry.sh".to_string(),
            license: "MIT".to_string(),
        },
        tier: Some("community".to_string()),
        maintainers: None,
        tags: None,
        compat: None,
        extra,
    }
}

fn manifest(packs: Vec<RegistryPackV1>) -> RegistryManifestV1 {
    RegistryManifestV1 {
        schema: "compas.registry.manifest.v1".to_string(),
        registry_version: "test".to_string(),
        archive: RegistryArchiveV1 {
            name: "registry.tar.gz".to_string(),
            sha256: "0".repeat(64),
        },
        plugins: vec![plugin("p1"), plugin("p2"), plugin("p3")],
        packs,
    }
}

#[test]
fn recommendations_are_priority_sorted_then_pack_id() {
    let manifest = manifest(vec![
        RegistryPackV1 {
            id: "beta".to_string(),
            description: "beta description".to_string(),
            plugins: vec!["p1".to_string()],
            capabilities: vec!["guard".to_string(), "lint".to_string()],
            requires: vec![],
            runtime_kind: "tool-backed".to_string(),
            cost_class: "medium".to_string(),
            recommendation: Some(RegistryPackRecommendationV1 {
                priority: 90,
                why: "Beta matches detected code languages.".to_string(),
                languages_any: vec!["rust".to_string()],
                languages_all: vec![],
                signals_any: vec![],
                signals_all: vec![],
                when_no_languages: false,
            }),
        },
        RegistryPackV1 {
            id: "alpha".to_string(),
            description: "alpha description".to_string(),
            plugins: vec!["p2".to_string()],
            capabilities: vec!["guard".to_string(), "lint".to_string()],
            requires: vec![],
            runtime_kind: "tool-backed".to_string(),
            cost_class: "medium".to_string(),
            recommendation: Some(RegistryPackRecommendationV1 {
                priority: 90,
                why: "Alpha matches detected code languages.".to_string(),
                languages_any: vec!["rust".to_string()],
                languages_all: vec![],
                signals_any: vec![],
                signals_all: vec![],
                when_no_languages: false,
            }),
        },
        RegistryPackV1 {
            id: "zeta".to_string(),
            description: "zeta description".to_string(),
            plugins: vec!["p3".to_string()],
            capabilities: vec!["guard".to_string(), "lint".to_string()],
            requires: vec![],
            runtime_kind: "tool-backed".to_string(),
            cost_class: "medium".to_string(),
            recommendation: Some(RegistryPackRecommendationV1 {
                priority: 100,
                why: "Zeta matches detected code languages.".to_string(),
                languages_any: vec!["rust".to_string()],
                languages_all: vec![],
                signals_any: vec![],
                signals_all: vec![],
                when_no_languages: false,
            }),
        },
    ]);

    let recommendations =
        recommendations_for_manifest_languages(&manifest, &[String::from("rust")], &[]);
    assert_eq!(
        recommendations
            .iter()
            .map(|item| item.pack_id.as_str())
            .collect::<Vec<_>>(),
        vec!["zeta", "alpha", "beta"]
    );
}

#[test]
fn recommendations_use_no_language_fallback_only_for_empty_language_set() {
    let manifest = manifest(vec![RegistryPackV1 {
        id: "starter-safe".to_string(),
        description: "starter description".to_string(),
        plugins: vec!["p1".to_string()],
        capabilities: vec!["guard".to_string(), "lint".to_string()],
        requires: vec![],
        runtime_kind: "tool-backed".to_string(),
        cost_class: "medium".to_string(),
        recommendation: Some(RegistryPackRecommendationV1 {
            priority: 95,
            why: "Safe default bundle when no supported language is detected.".to_string(),
            languages_any: vec![],
            languages_all: vec![],
            signals_any: vec![],
            signals_all: vec![],
            when_no_languages: true,
        }),
    }]);

    let no_languages = recommendations_for_manifest_languages(&manifest, &[], &[]);
    assert_eq!(no_languages.len(), 1);
    let rust_languages =
        recommendations_for_manifest_languages(&manifest, &[String::from("rust")], &[]);
    assert!(rust_languages.is_empty());
}

#[test]
fn recommendations_derive_pack_metadata_when_aggregate_fields_are_absent() {
    let mut derived = plugin("p1");
    derived
        .extra
        .insert("requires".to_string(), json!(["bootable"]));
    derived
        .extra
        .insert("runtime_kind".to_string(), json!("hybrid"));
    derived
        .extra
        .insert("cost_class".to_string(), json!("high"));

    let manifest = RegistryManifestV1 {
        schema: "compas.registry.manifest.v1".to_string(),
        registry_version: "test".to_string(),
        archive: RegistryArchiveV1 {
            name: "registry.tar.gz".to_string(),
            sha256: "0".repeat(64),
        },
        plugins: vec![derived],
        packs: vec![RegistryPackV1 {
            id: "quality".to_string(),
            description: "quality description".to_string(),
            plugins: vec!["p1".to_string()],
            capabilities: vec![],
            requires: vec![],
            runtime_kind: String::new(),
            cost_class: String::new(),
            recommendation: Some(RegistryPackRecommendationV1 {
                priority: 90,
                why: "Quality rails fit detected code languages.".to_string(),
                languages_any: vec!["rust".to_string()],
                languages_all: vec![],
                signals_any: vec![],
                signals_all: vec![],
                when_no_languages: false,
            }),
        }],
    };

    let recommendations =
        recommendations_for_manifest_languages(&manifest, &[String::from("rust")], &[]);
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].requires, vec!["bootable"]);
    assert_eq!(recommendations[0].runtime_kind, "hybrid");
    assert_eq!(recommendations[0].cost_class, "high");
}

#[test]
fn recommendations_match_repo_signals_and_report_which_matched() {
    let manifest = manifest(vec![RegistryPackV1 {
        id: "ai-proof-core".to_string(),
        description: "proof core description".to_string(),
        plugins: vec!["p1".to_string()],
        capabilities: vec!["guard".to_string(), "lint".to_string()],
        requires: vec![],
        runtime_kind: "tool-backed".to_string(),
        cost_class: "medium".to_string(),
        recommendation: Some(RegistryPackRecommendationV1 {
            priority: 100,
            why: "Proof kernel rails fit AI-first repos.".to_string(),
            languages_any: vec![],
            languages_all: vec![],
            signals_any: vec!["ai_first_scaffold".to_string()],
            signals_all: vec![],
            when_no_languages: false,
        }),
    }]);

    let recommendations = recommendations_for_manifest_languages(
        &manifest,
        &[String::from("rust")],
        &[String::from("ai_first_scaffold")],
    );
    assert_eq!(recommendations.len(), 1);
    assert_eq!(recommendations[0].matched_signals, ["ai_first_scaffold"]);
}

#[test]
fn detect_repo_signals_recognizes_ai_first_scaffold_and_runtime_markers() {
    let dir = tempfile::tempdir().unwrap();
    let plans_dir = dir.path().join("docs/exec-plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    let runtime_cfg = dir
        .path()
        .join(".agents/mcp/compas/runtime/worktree_isolation.toml");
    std::fs::create_dir_all(runtime_cfg.parent().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("AGENTS.md"),
        "# AGENTS\n\n<!-- compas:ai_first_router -->\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("ARCHITECTURE.md"), "# ARCHITECTURE\n").unwrap();
    std::fs::write(dir.path().join("docs/index.md"), "# Docs index\n").unwrap();
    std::fs::write(plans_dir.join("README.md"), "# Execution plans\n").unwrap();
    std::fs::write(plans_dir.join("TEMPLATE.md"), "# Template\n").unwrap();
    std::fs::write(
        dir.path().join("docs/QUALITY_SCORE.md"),
        "<!-- compas:quality_score:start -->\n...\n<!-- compas:quality_score:end -->\n",
    )
    .unwrap();
    std::fs::write(runtime_cfg, "schema = 'compas.worktree_isolation.v1'\n").unwrap();

    let signals = detect_repo_signals(dir.path());
    assert_eq!(
        signals,
        vec![
            "ai_first_scaffold".to_string(),
            "worktree_isolation_declared".to_string()
        ]
    );
}

#[test]
fn plan_init_ignores_registry_source_and_stays_advisory_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .unwrap();

    let plan_without_registry = plan_init(
        dir.path(),
        &InitRequest {
            repo_root: None,
            apply: Some(false),
            profile: None,
            registry_source: None,
            packs: None,
            external_packs: None,
        },
    )
    .unwrap();
    let plan_with_registry = plan_init(
        dir.path(),
        &InitRequest {
            repo_root: None,
            apply: Some(false),
            profile: None,
            registry_source: Some("https://example.com/registry.manifest.v1.json".to_string()),
            packs: None,
            external_packs: None,
        },
    )
    .unwrap();

    assert_eq!(plan_without_registry, plan_with_registry);
}

#[test]
fn manifest_validation_fails_closed_on_malformed_pack_recommendation() {
    let manifest = manifest(vec![RegistryPackV1 {
        id: "broken".to_string(),
        description: "broken description".to_string(),
        plugins: vec!["p1".to_string()],
        capabilities: vec!["guard".to_string(), "lint".to_string()],
        requires: vec![],
        runtime_kind: "tool-backed".to_string(),
        cost_class: "medium".to_string(),
        recommendation: Some(RegistryPackRecommendationV1 {
            priority: 10,
            why: "Broken recommendation is missing selectors.".to_string(),
            languages_any: vec![],
            languages_all: vec![],
            signals_any: vec![],
            signals_all: vec![],
            when_no_languages: false,
        }),
    }]);

    let err = validate_manifest_v1(&manifest).unwrap_err();
    assert!(
        err.contains("must define at least one selector"),
        "unexpected error: {err}"
    );
}

#[test]
fn manifest_validation_allows_prior_pack_shape_without_aggregate_metadata() {
    let manifest = manifest(vec![RegistryPackV1 {
        id: "prior".to_string(),
        description: "prior description".to_string(),
        plugins: vec!["p1".to_string()],
        capabilities: vec![],
        requires: vec![],
        runtime_kind: String::new(),
        cost_class: String::new(),
        recommendation: None,
    }]);

    validate_manifest_v1(&manifest).expect("older manifest shape should stay compatible");
}

#[test]
fn manifest_validation_allows_signal_selectors() {
    let manifest = manifest(vec![RegistryPackV1 {
        id: "proof".to_string(),
        description: "proof description".to_string(),
        plugins: vec!["p1".to_string()],
        capabilities: vec!["guard".to_string(), "lint".to_string()],
        requires: vec![],
        runtime_kind: "tool-backed".to_string(),
        cost_class: "medium".to_string(),
        recommendation: Some(RegistryPackRecommendationV1 {
            priority: 50,
            why: "Signal selectors are valid for AI-first repos.".to_string(),
            languages_any: vec![],
            languages_all: vec![],
            signals_any: vec!["ai_first_scaffold".to_string()],
            signals_all: vec![],
            when_no_languages: false,
        }),
    }]);

    validate_manifest_v1(&manifest).expect("signal selectors should validate");
}
