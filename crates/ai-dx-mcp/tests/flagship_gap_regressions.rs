use ai_dx_mcp::{
    api::{DecisionStatus, GateKind, ValidateMode, ViolationTier},
    app::{gate, validate},
};
use std::{
    path::Path,
    process::{Command, Stdio},
};

fn write_file(path: impl AsRef<Path>, content: &str) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

fn repo_root_str(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn setup_repo_with_suppressed_loc(repo_root: &Path) {
    write_file(
        repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "payload regression fixture"

[[tools]]
id = "noop"
description = "No-op gate tool"
command = "echo"
args = ["ok"]

[gate]
ci_fast = ["noop"]
ci = []
flagship = []

[[checks.loc]]
id = "loc-main"
max_loc = 1
include_globs = ["src/**/*.rs"]
exclude_globs = []
baseline_path = ".agents/mcp/compas/baselines/loc.json"
"#,
    );

    write_file(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999

[exceptions]
max_exceptions = 100
max_suppressed_ratio = 1.0
max_exception_window_days = 500000
"#,
    );

    write_file(
        repo_root.join(".agents/mcp/compas/allowlist.toml"),
        r#"
[[exceptions]]
id = "loc-suppress"
rule = "loc.max_exceeded"
path = "src/lib.rs"
owner = "qa"
reason = "payload regression fixture"
expires_at = "2999-01-01"
"#,
    );

    write_file(
        repo_root.join("src/lib.rs"),
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\n",
    );
}

fn setup_repo_for_change_impact_observation(repo_root: &Path) {
    write_file(
        repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "change_impact observation fixture"

[[tools]]
id = "noop"
description = "No-op gate tool"
command = "echo"
args = ["ok"]

[gate]
ci_fast = ["noop"]
ci = []
flagship = []
"#,
    );

    write_file(
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
unmapped_path_policy = "observe"

[[impact.rules]]
id = "mapped-rust"
path_globs = ["src/**/*.rs"]
required_tools = ["noop"]
"#,
    );

    write_file(repo_root.join("src/lib.rs"), "pub fn stable() {}\n");

    git(repo_root, &["init"]);
    git(repo_root, &["config", "user.email", "ci@example.com"]);
    git(repo_root, &["config", "user.name", "CI"]);
    git(repo_root, &["add", "."]);
    git(repo_root, &["commit", "-m", "initial"]);

    write_file(
        repo_root.join("docs/unmapped.md"),
        "# changed outside impact mapping\n",
    );
    git(repo_root, &["add", "."]);
    git(repo_root, &["commit", "-m", "add unmapped docs change"]);
}

fn setup_repo_for_stderr_pattern(repo_root: &Path) {
    write_file(
        repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "stderr receipt matcher fixture"

[[tools]]
id = "stderr-tool"
description = "writes marker to stderr"
command = "python3"
args = ["-c", "import sys; sys.stderr.write('MATCH_STDERR_TOKEN\\n')"]
receipt_contract = { expect_stdout_pattern = "MATCH_STDERR_TOKEN" }

[gate]
ci_fast = ["stderr-tool"]
ci = []
flagship = []
"#,
    );

    write_file(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999
"#,
    );
}

fn setup_repo_for_flagship_gate(repo_root: &Path) {
    write_file(
        repo_root.join(".agents/mcp/compas/plugins/default/plugin.toml"),
        r#"
[plugin]
id = "default"
description = "flagship gate fixture"

[[tools]]
id = "flagship-tool"
description = "writes flagship marker"
command = "python3"
args = ["-c", "print('FLAGSHIP_OK')"]
receipt_contract = { expect_stdout_pattern = "FLAGSHIP_OK" }

[gate]
ci_fast = []
ci = []
flagship = ["flagship-tool"]
"#,
    );

    write_file(
        repo_root.join(".agents/mcp/compas/quality_contract.toml"),
        r#"
[quality]
min_trust_score = 0
min_coverage_percent = 0.0
allow_trust_drop = true
allow_coverage_drop = true
max_weighted_risk_increase = 999
"#,
    );
}

fn git(repo_root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn payload_has_suppression_context_in_verdict_and_agent_digest() {
    let dir = tempfile::tempdir().expect("temp repo");
    setup_repo_with_suppressed_loc(dir.path());
    let repo_root = repo_root_str(dir.path());

    let validate_out = validate(&repo_root, ValidateMode::Ratchet, false, None);
    assert!(
        validate_out.ok,
        "validate fixture must pass in ratchet mode: error={:?}, violations={:?}, verdict={:?}",
        validate_out.error,
        validate_out
            .violations
            .iter()
            .map(|v| v.code.clone())
            .collect::<Vec<_>>(),
        validate_out.verdict
    );
    assert!(
        !validate_out.suppressed.is_empty(),
        "fixture must produce suppressed violations"
    );

    let gate_out = gate(&repo_root, GateKind::CiFast, true, false).await;
    assert!(gate_out.ok, "gate fixture must pass: {:?}", gate_out.error);
    assert!(
        !gate_out.validate.suppressed.is_empty(),
        "gate payload must keep validate.suppressed"
    );

    let validate_payload = serde_json::to_value(&validate_out).expect("validate payload json");
    let gate_payload = serde_json::to_value(&gate_out).expect("gate payload json");

    let validate_verdict = validate_out.verdict.as_ref().expect("validate verdict");
    assert!(
        validate_verdict.quality_posture.is_some(),
        "validate verdict must include quality_posture"
    );
    let validate_raw_trust = validate_verdict
        .quality_posture
        .as_ref()
        .expect("quality_posture just checked")
        .trust_score;
    let validate_display_trust = validate_out
        .trust_score
        .as_ref()
        .expect("display trust_score must be present")
        .score;
    assert!(
        validate_display_trust >= validate_raw_trust,
        "display trust must not be lower than raw trust after suppression"
    );
    assert_ne!(
        validate_display_trust, validate_raw_trust,
        "raw and display trust should diverge when violations are suppressed"
    );
    assert_eq!(
        validate_verdict.suppressed_count,
        validate_out.suppressed.len(),
        "validate verdict suppressed_count must match payload.suppressed"
    );
    assert!(
        validate_verdict
            .suppressed_codes
            .iter()
            .any(|code| code == "loc.max_exceeded"),
        "validate verdict must include suppressed code summary"
    );

    let validate_digest = validate_out
        .agent_digest
        .as_ref()
        .expect("validate agent_digest");
    assert_eq!(
        validate_digest.suppressed_count,
        validate_out.suppressed.len(),
        "validate agent_digest suppressed_count must match payload.suppressed"
    );
    assert!(
        validate_digest
            .suppressed_top_codes
            .iter()
            .any(|code| code == "loc.max_exceeded"),
        "validate agent_digest must include suppressed summary"
    );

    let gate_verdict = gate_out.verdict.as_ref().expect("gate verdict");
    assert!(
        gate_verdict.quality_posture.is_some(),
        "gate verdict must include quality_posture"
    );
    let gate_raw_trust = gate_verdict
        .quality_posture
        .as_ref()
        .expect("quality_posture just checked")
        .trust_score;
    let gate_display_trust = gate_out
        .validate
        .trust_score
        .as_ref()
        .expect("gate.validate display trust_score must be present")
        .score;
    assert!(
        gate_display_trust >= gate_raw_trust,
        "gate display trust must not be lower than raw trust after suppression"
    );
    assert_ne!(
        gate_display_trust, gate_raw_trust,
        "gate raw and display trust should diverge when violations are suppressed"
    );
    assert_eq!(
        gate_verdict.suppressed_count,
        gate_out.validate.suppressed.len(),
        "gate verdict suppressed_count must match gate.validate.suppressed"
    );
    assert!(
        gate_verdict
            .suppressed_codes
            .iter()
            .any(|code| code == "loc.max_exceeded"),
        "gate verdict must include suppressed code summary"
    );

    let gate_digest = gate_out.agent_digest.as_ref().expect("gate agent_digest");
    assert_eq!(
        gate_digest.suppressed_count,
        gate_out.validate.suppressed.len(),
        "gate agent_digest suppressed_count must match gate.validate.suppressed"
    );
    assert!(
        gate_digest
            .suppressed_top_codes
            .iter()
            .any(|code| code == "loc.max_exceeded"),
        "gate agent_digest must include suppressed summary"
    );

    assert!(
        validate_payload
            .pointer("/verdict/quality_posture")
            .is_some(),
        "validate payload JSON must expose verdict.quality_posture"
    );
    assert!(
        validate_payload
            .pointer("/verdict/suppressed_count")
            .is_some(),
        "validate payload JSON must expose verdict.suppressed_count"
    );
    assert!(
        validate_payload
            .pointer("/agent_digest/suppressed_top_codes")
            .is_some(),
        "validate payload JSON must expose agent_digest suppressed summary"
    );

    assert!(
        gate_payload.pointer("/verdict/quality_posture").is_some(),
        "gate payload JSON must expose verdict.quality_posture"
    );
    assert!(
        gate_payload.pointer("/verdict/suppressed_count").is_some(),
        "gate payload JSON must expose verdict.suppressed_count"
    );
    assert!(
        gate_payload
            .pointer("/agent_digest/suppressed_top_codes")
            .is_some(),
        "gate payload JSON must expose agent_digest suppressed summary"
    );
}

#[tokio::test]
async fn change_impact_observe_policy_marks_unmapped_path_as_observation() {
    let dir = tempfile::tempdir().expect("temp repo");
    setup_repo_for_change_impact_observation(dir.path());
    let repo_root = repo_root_str(dir.path());

    let out = gate(&repo_root, GateKind::CiFast, true, false).await;
    let verdict = out.verdict.expect("verdict");

    let reason = verdict
        .decision
        .reasons
        .iter()
        .find(|r| r.code == "change_impact.unmapped_path");

    assert!(
        reason.is_some(),
        "unmapped path must be surfaced as observation in gate verdict; status={:?}, reasons={:?}, error={:?}",
        verdict.decision.status,
        verdict
            .decision
            .reasons
            .iter()
            .map(|r| (&r.code, &r.tier))
            .collect::<Vec<_>>(),
        out.error
    );

    let reason = reason.expect("reason just checked");
    assert_eq!(
        reason.tier,
        ViolationTier::Observation,
        "change_impact.unmapped_path must be observation"
    );
    assert_eq!(
        verdict.decision.status,
        DecisionStatus::Pass,
        "observation-only unmapped path must not block gate"
    );
}

#[tokio::test]
async fn receipt_pattern_contract_accepts_match_from_stderr_tail() {
    let dir = tempfile::tempdir().expect("temp repo");
    setup_repo_for_stderr_pattern(dir.path());
    let repo_root = repo_root_str(dir.path());

    let out = gate(&repo_root, GateKind::CiFast, false, false).await;
    let verdict = out.verdict.clone().expect("verdict");

    assert_eq!(out.receipts.len(), 1, "expected one gate receipt");
    assert!(
        out.receipts[0].stderr_tail.contains("MATCH_STDERR_TOKEN"),
        "fixture must produce stderr marker"
    );
    assert!(out.ok, "gate should pass when pattern matches stderr_tail");
    assert_eq!(
        verdict.decision.status,
        DecisionStatus::Pass,
        "stderr pattern match should keep gate pass"
    );
    assert!(
        !verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.receipt_contract_violated"),
        "receipt matcher must not report contract violation on stderr-tail match"
    );
}

#[tokio::test]
async fn flagship_gate_executes_tools_and_keeps_verdict_consistent() {
    let dir = tempfile::tempdir().expect("temp repo");
    setup_repo_for_flagship_gate(dir.path());
    let repo_root = repo_root_str(dir.path());

    let out = gate(&repo_root, GateKind::Flagship, false, false).await;
    let verdict = out.verdict.clone().expect("verdict");

    assert_eq!(out.receipts.len(), 1, "expected one flagship receipt");
    assert_eq!(
        out.receipts[0].tool_id, "flagship-tool",
        "flagship gate must execute flagship tool sequence"
    );
    assert!(
        out.receipts[0].stdout_tail.contains("FLAGSHIP_OK"),
        "flagship fixture must emit stdout marker"
    );
    assert!(out.ok, "flagship gate should pass on successful tool run");
    assert_eq!(
        verdict.decision.status,
        DecisionStatus::Pass,
        "successful flagship run must keep pass verdict"
    );
    assert!(
        !verdict
            .decision
            .reasons
            .iter()
            .any(|r| r.code == "gate.empty_sequence"),
        "flagship gate must not degrade into empty-sequence path"
    );
}
