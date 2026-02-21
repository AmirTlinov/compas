use ai_dx_mcp::{
    api::{DecisionStatus, GateKind},
    app::gate,
};
use std::{path::Path, process::Command, process::Stdio};
use tempfile::tempdir;

const SBOM_TOOL_TOML: &str = include_str!("../../../tools/custom/sbom/tool.toml");
const SBOM_TOOL_SCRIPT: &str = include_str!("../../../tools/custom/sbom/run_sbom.py");

fn write(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, content).expect("write test file");
}

fn git(repo_root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("run git");
    if !out.status.success() {
        panic!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn setup_repo_with_gate(repo_root: &Path, include_sbom_in_gate: bool) {
    write(
        repo_root.join(".agents/mcp/compas/plugins/p10/plugin.toml"),
        &format!(
            r#"
[plugin]
id = "p10"
description = "SBOM integration fixture plugin for P10"
tool_import_globs = ["tools/custom/**/tool.toml"]

[[tools]]
id = "noop"
description = "No-op gate helper"
command = "echo"
args = ["noop"]

[gate]
ci_fast = [{}]
ci = []
flagship = []
"#,
            if include_sbom_in_gate {
                "\"sbom\""
            } else {
                "\"noop\""
            }
        ),
    );

    write(
        repo_root.join("tools/custom/sbom/tool.toml"),
        SBOM_TOOL_TOML,
    );
    write(
        repo_root.join("tools/custom/sbom/run_sbom.py"),
        SBOM_TOOL_SCRIPT,
    );

    write(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999

[impact]
diff_base = "HEAD~1"
unmapped_path_policy = "ignore"

[[impact.rules]]
id = "dependency-manifests"
path_globs = ["Cargo.toml", "Cargo.lock"]
required_tools = ["sbom"]
"#,
    );

    git(repo_root, &["init"]);
    git(repo_root, &["config", "user.email", "ci@example.com"]);
    git(repo_root, &["config", "user.name", "CI"]);
    git(repo_root, &["add", "."]);
    git(repo_root, &["commit", "-m", "initial"]);
}

#[tokio::test]
async fn p10_sbom_gate_passes_when_manifest_changes_include_sbom_in_ci_fast() {
    let dir = tempdir().expect("temp repo");
    setup_repo_with_gate(dir.path(), true);

    write(dir.path().join("Cargo.toml"), "name = \"p10-fixture\"\n");
    git(dir.path(), &["add", "Cargo.toml"]);
    git(dir.path(), &["commit", "-m", "add manifest"]);

    let out = gate(&dir.path().to_string_lossy(), GateKind::CiFast, false, false).await;
    let verdict = out.verdict.clone().expect("verdict");

    assert!(
        out.ok,
        "gate blocked: {:?}; reasons={:?}",
        out.error,
        verdict.decision.reasons
    );
    assert_eq!(verdict.decision.status, DecisionStatus::Pass);
    assert!(
        !out
            .verdict
            .as_ref()
            .expect("verdict")
            .decision
            .reasons
            .iter()
            .any(|reason| reason.code == "change_impact.required_tool_missing"),
        "sbom-required change must not produce required_tool_missing: {:?}",
        verdict.decision.reasons
    );
    assert!(
        out.receipts
            .iter()
            .any(|receipt| receipt.stdout_tail.contains("SBOM_OK")),
        "sbom tool must execute and emit SBOM_OK marker"
    );
}

#[tokio::test]
async fn p10_sbom_gate_blocks_when_manifest_change_lacks_required_sbom_tool() {
    let dir = tempdir().expect("temp repo");
    setup_repo_with_gate(dir.path(), false);

    write(dir.path().join("Cargo.toml"), "name = \"p10-fixture\"\n");
    git(dir.path(), &["add", "Cargo.toml"]);
    git(dir.path(), &["commit", "-m", "add manifest"]);

    let out = gate(&dir.path().to_string_lossy(), GateKind::CiFast, false, false).await;
    let verdict = out.verdict.clone().expect("verdict");

    assert!(
        !out.ok,
        "gate must fail when required impact tool is missing from gate sequence"
    );
    assert_eq!(
        verdict.decision.status,
        DecisionStatus::Blocked,
        "missing required impact tool must be blocking"
    );
    assert!(
        verdict
            .decision
            .reasons
            .iter()
            .any(|reason| reason.code == "change_impact.required_tool_missing"),
        "expected required_tool_missing for manifest-only change, got {:?}",
        verdict.decision.reasons
    );
    assert_ne!(
        out.error.as_ref().map(|e| e.code.as_str()),
        Some("gate.empty_sequence"),
        "failure must be due to policy mapping, not missing gate sequence"
    );
}
