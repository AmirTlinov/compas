use ai_dx_mcp::api::InitOutput;

#[test]
fn cli_init_dry_run_and_apply_smoke() {
    let dir = tempfile::tempdir().expect("temp repo");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");

    let out = std::process::Command::new(bin)
        .args(["init", "--repo-root"])
        .arg(dir.path())
        .output()
        .expect("run init");
    assert!(
        out.status.success(),
        "init dry-run failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: InitOutput = serde_json::from_slice(&out.stdout).expect("parse InitOutput");
    assert!(parsed.ok, "ok=false; error={:?}", parsed.error);
    assert!(!parsed.applied, "dry-run should not apply");
    assert!(parsed.plan.is_some(), "plan missing");

    let out = std::process::Command::new(bin)
        .args(["init", "--apply", "--repo-root"])
        .arg(dir.path())
        .output()
        .expect("run init --apply");
    assert!(
        out.status.success(),
        "init apply failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: InitOutput = serde_json::from_slice(&out.stdout).expect("parse InitOutput");
    assert!(parsed.ok, "ok=false; error={:?}", parsed.error);
    assert!(parsed.applied, "apply=true expected");
    let plan = parsed.plan.expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.content_utf8.contains("omitted")),
        "expected apply output to redact contents"
    );
    assert!(
        dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml")
            .is_file(),
        "plugin.toml not created"
    );
}

#[test]
fn cli_validate_rejects_unknown_flags() {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .args(["validate", "ratchet", "--typo"])
        .output()
        .expect("run validate with typo");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown"),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_v1_flags_are_rejected() {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .args(["--validate", "ratchet"])
        .output()
        .expect("run v1-style --validate");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("v1-style CLI flag"),
        "stderr missing migration hint: {stderr}"
    );
}

#[test]
fn cli_help_prints_usage() {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let out = std::process::Command::new(bin)
        .args(["--help"])
        .output()
        .expect("run --help");
    assert!(
        out.status.success(),
        "help failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Usage:"), "stdout={stdout}");
    assert!(stdout.contains("validate"), "stdout={stdout}");
    assert!(stdout.contains("AI_DX_REPO_ROOT"), "stdout={stdout}");
}

#[test]
fn cli_validate_write_baseline_ratchet_requires_and_accepts_maintenance() {
    let dir = tempfile::tempdir().expect("temp repo");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");

    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");

    // Bootstrap compas config.
    let init = std::process::Command::new(bin)
        .args(["init", "--apply", "--repo-root"])
        .arg(dir.path())
        .output()
        .expect("run init --apply");
    assert!(
        init.status.success(),
        "init apply failed: stderr={}",
        String::from_utf8_lossy(&init.stderr)
    );
    std::fs::write(dir.path().join("Cargo.lock"), "# lock").expect("write Cargo.lock");

    // Ratchet baseline write without maintenance must fail-closed.
    let out = std::process::Command::new(bin)
        .args(["validate", "ratchet", "--write-baseline", "--repo-root"])
        .arg(dir.path())
        .output()
        .expect("run validate ratchet --write-baseline without maintenance");
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out_json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("parse validate output");
    assert_eq!(
        out_json
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|v| v.as_str()),
        Some("config.baseline_write_requires_maintenance")
    );

    // With maintenance metadata the same flow should pass.
    let out = std::process::Command::new(bin)
        .args([
            "validate",
            "ratchet",
            "--write-baseline",
            "--baseline-reason",
            "Quarterly baseline refresh after major policy changes",
            "--baseline-owner",
            "team-lead",
            "--repo-root",
        ])
        .arg(dir.path())
        .output()
        .expect("run validate ratchet --write-baseline with maintenance");
    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let out_json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("parse validate output");
    assert_eq!(out_json.get("ok").and_then(|v| v.as_bool()), Some(true));
}
