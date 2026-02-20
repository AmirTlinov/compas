use super::{plan_init, resolve_gate_tools_for_packs};
use crate::api::{CanonicalToolId, CanonicalToolsConfig, GateKind, InitRequest, ValidateMode};
use crate::config::PluginConfig;
use crate::config::ProjectTool;
use crate::packs::schema::{PackGatesV1, PackManifestV1, PackMetaV1, PackToolTemplateV1};
use std::collections::BTreeMap;
use std::fs;
use tempfile::tempdir;

fn write(repo: &std::path::Path, rel: &str, content: &str) {
    let path = repo.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn init_req() -> InitRequest {
    InitRequest {
        repo_root: None,
        apply: Some(false),
        packs: None,
        external_packs: None,
    }
}

#[test]
fn plan_init_detects_each_builtin_language_pack() {
    let cases = [
        ("Cargo.toml", "rust", "tools/custom/rust-test/tool.toml"),
        (
            "pyproject.toml",
            "python",
            "tools/custom/python-test/tool.toml",
        ),
        ("go.mod", "go", "tools/custom/go-test/tool.toml"),
        (
            "CMakeLists.txt",
            "cmake",
            "tools/custom/cmake-configure/tool.toml",
        ),
        (
            "global.json",
            "dotnet",
            "tools/custom/dotnet-test/tool.toml",
        ),
    ];

    for (marker, pack_id, tool_path) in cases {
        let dir = tempdir().unwrap();
        write(dir.path(), marker, "x");

        let plan = plan_init(dir.path(), &init_req()).expect("plan");
        assert!(
            plan.writes
                .iter()
                .any(|w| w.path == format!(".agents/mcp/compas/packs/{pack_id}/pack.toml")),
            "marker={marker} should select pack={pack_id}"
        );
        assert!(
            plan.writes.iter().any(|w| w.path == tool_path),
            "marker={marker} should include tool manifest {tool_path}"
        );
    }
}

#[test]
fn plan_init_unknown_repo_still_scaffolds_generic_config() {
    let dir = tempdir().unwrap();
    write(dir.path(), "README.md", "# docs-only repo");

    let plan = plan_init(dir.path(), &init_req()).expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/plugins/default/plugin.toml"),
        "init must scaffold plugin even when no builtin packs are detected"
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/quality_contract.toml"),
        "init must scaffold quality contract for unknown repos"
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs.lock"),
        "init must scaffold packs.lock for unknown repos"
    );
}

#[test]
fn plan_init_detects_node_npm_pack_and_tool_manifest() {
    let dir = tempdir().unwrap();
    write(dir.path(), "package.json", "{}");
    write(dir.path(), "package-lock.json", "{}");

    let plan = plan_init(dir.path(), &init_req()).expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/node-npm/pack.toml"),
        "node-npm pack must be selected for npm lockfile project"
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == "tools/custom/npm-test/tool.toml"),
        "npm-test tool manifest must be generated"
    );
}

#[test]
fn plan_init_detects_node_bun_pack_and_tool_manifest() {
    let dir = tempdir().unwrap();
    write(dir.path(), "package.json", "{}");
    write(dir.path(), "bun.lock", "");

    let plan = plan_init(dir.path(), &init_req()).expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/node-bun/pack.toml"),
        "node-bun pack must be selected for bun lockfile project"
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == "tools/custom/bun-test/tool.toml"),
        "bun-test tool manifest must be generated"
    );
    assert!(
        !plan
            .writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/node-npm/pack.toml"),
        "node-npm must not be selected when bun lockfile is present"
    );
}

#[test]
fn plan_init_detects_dotnet_pack_from_csproj_or_sln_without_global_json() {
    let cases = [
        ("src/App/App.csproj", "<Project />"),
        ("workspace.sln", "Microsoft Visual Studio Solution File"),
    ];

    for (marker, content) in cases {
        let dir = tempdir().unwrap();
        write(dir.path(), marker, content);

        let plan = plan_init(dir.path(), &init_req()).expect("plan");
        assert!(
            plan.writes
                .iter()
                .any(|w| w.path == ".agents/mcp/compas/packs/dotnet/pack.toml"),
            "marker={marker} should select dotnet pack without global.json"
        );
        assert!(
            plan.writes
                .iter()
                .any(|w| w.path == "tools/custom/dotnet-test/tool.toml"),
            "marker={marker} should generate dotnet-test tool"
        );
    }
}

#[test]
fn plan_init_prefers_pytest_pack_over_generic_python_when_pytest_config_present() {
    let dir = tempdir().unwrap();
    write(dir.path(), "pyproject.toml", "[project]\nname='x'\n");
    write(dir.path(), "pytest.ini", "[pytest]\n");

    let plan = plan_init(dir.path(), &init_req()).expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/python-pytest/pack.toml"),
        "python-pytest pack must be selected"
    );
    assert!(
        !plan
            .writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/python/pack.toml"),
        "generic python pack must be skipped when pytest config is present"
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == "tools/custom/pytest-test/tool.toml"),
        "pytest-test tool must be generated"
    );
    assert!(
        !plan
            .writes
            .iter()
            .any(|w| w.path == "tools/custom/python-test/tool.toml"),
        "python-test tool must be skipped when pytest config is present"
    );
}

#[test]
fn plan_init_wires_minimum_quality_checks_by_default() {
    let dir = tempdir().unwrap();
    write(dir.path(), "Cargo.toml", "x");

    let plan = plan_init(dir.path(), &init_req()).expect("plan");
    let plugin = plan
        .writes
        .iter()
        .find(|w| w.path == ".agents/mcp/compas/plugins/default/plugin.toml")
        .expect("plugin write");
    let parsed: PluginConfig = toml::from_str(&plugin.content_utf8).expect("parse plugin.toml");
    let checks = parsed.checks.expect("checks must be present");
    assert!(!checks.loc.is_empty(), "loc check must be wired");
    assert!(
        !checks.supply_chain.is_empty(),
        "supply_chain check must be wired"
    );
    assert!(
        !checks.tool_budget.is_empty(),
        "tool_budget check must be wired"
    );
}

#[test]
fn plan_init_wires_polyglot_gate_deterministically() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    write(repo, "Cargo.toml", "x");
    write(repo, "pyproject.toml", "x");
    write(repo, "go.mod", "x");
    write(repo, "CMakeLists.txt", "x");
    write(repo, "global.json", "x");
    write(repo, "package.json", "{}");
    write(repo, "package-lock.json", "{}");

    let plan = plan_init(repo, &init_req()).expect("plan");
    let plugin = plan
        .writes
        .iter()
        .find(|w| w.path == ".agents/mcp/compas/plugins/default/plugin.toml")
        .expect("plugin write");
    let parsed: PluginConfig = toml::from_str(&plugin.content_utf8).expect("parse plugin.toml");
    let gate = parsed.gate.expect("gate");

    let expected = vec![
        "cmake-configure",
        "cmake-build",
        "cmake-test",
        "dotnet-test",
        "go-test",
        "npm-test",
        "python-test",
        "rust-test",
    ];
    assert_eq!(gate.ci_fast, expected);
    assert_eq!(gate.ci, gate.ci_fast);
    assert_eq!(gate.flagship, gate.ci_fast);
}

#[test]
fn init_apply_is_idempotent_and_validate_ok() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    write(repo, "Cargo.toml", "x");
    write(repo, "Cargo.lock", "# lock");
    let repo_root = repo.to_string_lossy().to_string();

    let out1 = crate::init::init(
        &repo_root,
        InitRequest {
            repo_root: Some(repo_root.clone()),
            apply: Some(true),
            packs: None,
            external_packs: None,
        },
    );
    assert!(out1.ok, "out1 ok=false; error={:?}", out1.error);
    assert!(out1.applied);

    let out2 = crate::init::init(
        &repo_root,
        InitRequest {
            repo_root: Some(repo_root.clone()),
            apply: Some(true),
            packs: None,
            external_packs: None,
        },
    );
    assert!(out2.ok, "out2 ok=false; error={:?}", out2.error);
    assert!(out2.applied);

    let validate = crate::app::validate(&repo_root, ValidateMode::Ratchet, false, None);
    assert!(
        validate.ok,
        "validate ok=false; violations={:?}",
        validate.violations
    );
}

#[test]
fn init_apply_fails_closed_on_conflicting_existing_file() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    write(repo, "Cargo.toml", "x");
    write(
        repo,
        ".agents/mcp/compas/plugins/default/plugin.toml",
        "# user config\n",
    );
    let repo_root = repo.to_string_lossy().to_string();

    let out = crate::init::init(
        &repo_root,
        InitRequest {
            repo_root: Some(repo_root.clone()),
            apply: Some(true),
            packs: None,
            external_packs: None,
        },
    );
    assert!(!out.ok, "expected ok=false");
    let err = out.error.expect("error");
    assert_eq!(err.code, "init.write_conflict", "wrong code: {err:?}");

    // Must not clobber user content.
    let still = fs::read_to_string(repo.join(".agents/mcp/compas/plugins/default/plugin.toml"))
        .expect("read plugin.toml");
    assert!(
        still.contains("user config"),
        "file was clobbered: {still:?}"
    );
    assert!(
        !repo.join("tools/custom/rust-test/tool.toml").exists(),
        "should not partially apply writes on conflict"
    );
}

#[test]
fn resolve_gate_tools_maps_canonical_ids_and_honors_gate_kinds() {
    let tool = |id: &str| ProjectTool {
        id: id.to_string(),
        description: format!("tool {id}"),
        command: "echo".to_string(),
        args: vec!["ok".to_string()],
        cwd: None,
        timeout_ms: None,
        max_stdout_bytes: None,
        max_stderr_bytes: None,
        receipt_contract: None,
        env: BTreeMap::new(),
    };

    let pack = PackManifestV1 {
        pack: PackMetaV1 {
            id: "x".to_string(),
            version: "0.1.0".to_string(),
            description: "x".to_string(),
            languages: vec![],
        },
        detectors: vec![],
        tools: vec![
            PackToolTemplateV1 {
                tool: tool("t-build"),
            },
            PackToolTemplateV1 {
                tool: tool("t-test"),
            },
        ],
        canonical_tools: Some(CanonicalToolsConfig {
            build: vec!["t-build".to_string()],
            test: vec!["t-test".to_string()],
            disabled: vec![
                CanonicalToolId::Lint,
                CanonicalToolId::Fmt,
                CanonicalToolId::Docs,
            ],
            ..Default::default()
        }),
        gates: Some(PackGatesV1 {
            ci_fast: vec![CanonicalToolId::Test],
            ci: vec![CanonicalToolId::Build, CanonicalToolId::Test],
            flagship: vec![CanonicalToolId::Build],
        }),
        checks_v2: None,
    };

    let ci_fast =
        resolve_gate_tools_for_packs(std::slice::from_ref(&pack), GateKind::CiFast).unwrap();
    assert_eq!(ci_fast, vec!["t-test"]);

    let ci = resolve_gate_tools_for_packs(std::slice::from_ref(&pack), GateKind::Ci).unwrap();
    assert_eq!(ci, vec!["t-build", "t-test"]);

    let flagship = resolve_gate_tools_for_packs(&[pack], GateKind::Flagship).unwrap();
    assert_eq!(flagship, vec!["t-build"]);
}

#[test]
fn init_e2e_polyglot_validate_then_gate_ci_fast_dry_run_ok() {
    let dir = tempdir().unwrap();
    let repo = dir.path();

    write(repo, "Cargo.toml", "x");
    write(repo, "Cargo.lock", "# lock");
    write(repo, "pyproject.toml", "x");
    write(repo, "poetry.lock", "# lock");
    write(repo, "go.mod", "x");
    write(repo, "CMakeLists.txt", "x");
    write(repo, "global.json", "x");
    write(repo, "package.json", "{}");
    write(repo, "package-lock.json", "{}");

    let repo_root = repo.to_string_lossy().to_string();
    let out = crate::init::init(
        &repo_root,
        InitRequest {
            repo_root: Some(repo_root.clone()),
            apply: Some(true),
            packs: None,
            external_packs: None,
        },
    );
    assert!(out.ok, "init ok=false; error={:?}", out.error);

    let validate = crate::app::validate(&repo_root, ValidateMode::Ratchet, false, None);
    assert!(
        validate.ok,
        "validate ok=false; violations={:?}",
        validate.violations
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let gate = rt.block_on(crate::app::gate(
        &repo_root,
        crate::api::GateKind::CiFast,
        true,
        false,
    ));
    assert!(gate.ok, "gate ok=false; error={:?}", gate.error);
    assert_eq!(
        gate.receipts
            .iter()
            .map(|r| r.tool_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "cmake-configure",
            "cmake-build",
            "cmake-test",
            "dotnet-test",
            "go-test",
            "npm-test",
            "python-test",
            "rust-test",
        ]
    );
}
