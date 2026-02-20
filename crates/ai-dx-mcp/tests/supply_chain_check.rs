use ai_dx_mcp::{checks::supply_chain::run_supply_chain_check, config::SupplyChainCheckConfigV2};
use tempfile::tempdir;

fn cfg() -> SupplyChainCheckConfigV2 {
    SupplyChainCheckConfigV2 {
        id: "supply-chain".to_string(),
    }
}

#[test]
fn supply_chain_requires_cargo_lock_when_rust_manifest_exists() {
    let dir = tempdir().expect("temp dir");
    let repo = dir.path();
    std::fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "x"
version = "0.1.0"
"#,
    )
    .expect("write Cargo.toml");

    let out = run_supply_chain_check(repo, &cfg());
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "supply_chain.lockfile_missing"),
        "{:?}",
        out.violations
    );
}

#[test]
fn supply_chain_allows_rust_manifest_with_lockfile() {
    let dir = tempdir().expect("temp dir");
    let repo = dir.path();
    std::fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "x"
version = "0.1.0"
"#,
    )
    .expect("write Cargo.toml");
    std::fs::write(repo.join("Cargo.lock"), "# lock").expect("write Cargo.lock");

    let out = run_supply_chain_check(repo, &cfg());
    assert!(
        !out.violations
            .iter()
            .any(|v| v.code == "supply_chain.lockfile_missing"),
        "{:?}",
        out.violations
    );
}

#[test]
fn supply_chain_detects_prerelease_rust_dependency() {
    let dir = tempdir().expect("temp dir");
    let repo = dir.path();
    std::fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "x"
version = "0.1.0"

[dependencies]
foo = "1.2.3-rc.1"
"#,
    )
    .expect("write Cargo.toml");
    std::fs::write(repo.join("Cargo.lock"), "# lock").expect("write Cargo.lock");

    let out = run_supply_chain_check(repo, &cfg());
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "supply_chain.prerelease_dependency"),
        "{:?}",
        out.violations
    );
}

#[test]
fn supply_chain_detects_prerelease_node_dependency() {
    let dir = tempdir().expect("temp dir");
    let repo = dir.path();
    std::fs::write(
        repo.join("package.json"),
        r#"{
  "name": "x",
  "version": "0.1.0",
  "dependencies": {
    "left-pad": "2.0.0-beta.1"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(repo.join("package-lock.json"), "{}").expect("write lock");

    let out = run_supply_chain_check(repo, &cfg());
    assert!(
        out.violations
            .iter()
            .any(|v| v.code == "supply_chain.prerelease_dependency"),
        "{:?}",
        out.violations
    );
}
