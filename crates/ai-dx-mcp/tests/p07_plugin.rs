use std::{fs, path::PathBuf};

use ai_dx_mcp::config::{PluginConfig, QualityContractConfig};

#[test]
fn p07_plugin_owns_dead_code_and_orphan_api_checks() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();

    let default_plugin: PluginConfig = toml::from_str(
        &fs::read_to_string(repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"))
            .expect("default plugin toml"),
    )
    .expect("parse default plugin");
    let p07_plugin: PluginConfig = toml::from_str(
        &fs::read_to_string(repo_root.join(".agents/mcp/compas/plugins/p07/plugin.toml"))
            .expect("p07 plugin toml"),
    )
    .expect("parse p07 plugin");

    assert_eq!(default_plugin.plugin.id, "default");
    assert_eq!(p07_plugin.plugin.id, "p07");

    let default_checks = default_plugin.checks.expect("default checks");
    assert!(
        default_checks.dead_code.is_empty(),
        "default should not own dead_code checks"
    );
    assert!(
        default_checks.orphan_api.is_empty(),
        "default should not own orphan_api checks"
    );

    let p07_checks = p07_plugin.checks.expect("p07 checks");
    assert_eq!(p07_checks.dead_code.len(), 1);
    assert_eq!(p07_checks.orphan_api.len(), 1);
    assert_eq!(p07_checks.dead_code[0].id, "p07-dead-code-main");
    assert_eq!(p07_checks.orphan_api[0].id, "p07-orphan-api-main");

    let quality_contract: QualityContractConfig = toml::from_str(
        &fs::read_to_string(repo_root.join(".agents/mcp/compas/quality_contract.toml"))
            .expect("read quality_contract"),
    )
    .expect("parse quality contract");
    assert!(
        quality_contract
            .governance
            .mandatory_checks
            .iter()
            .any(|name| name == "dead_code")
    );
    assert!(
        quality_contract
            .governance
            .mandatory_checks
            .iter()
            .any(|name| name == "orphan_api")
    );
}
