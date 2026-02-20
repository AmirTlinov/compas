use ai_dx_mcp::{
    api::{InitOutput, ToolsListOutput, ValidateMode, ValidateOutput},
    server::AiDxServer,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn repo_root_from_manifest_dir() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // .../crates/ai-dx-mcp -> repo root is 2 levels up.
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf()
}

#[tokio::test]
async fn mcp_smoke_list_tools_and_validate_warn() {
    let repo_root = repo_root_from_manifest_dir();
    let repo_root_str = repo_root.to_string_lossy().to_string();

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move { AiDxServer::new().serve(server_io).await });
    let mut client = ().serve(client_io).await.expect("serve client");
    let mut server = server_task
        .await
        .expect("join server task")
        .expect("serve server");

    let tools = client
        .list_tools(Default::default())
        .await
        .expect("list tools");
    assert!(tools.tools.iter().any(|t| t.name == "compas.validate"));
    assert!(tools.tools.iter().any(|t| t.name == "compas.gate"));
    assert!(tools.tools.iter().any(|t| t.name == "compas.init"));
    assert!(tools.tools.iter().any(|t| t.name == "compas.catalog"));
    assert!(tools.tools.iter().any(|t| t.name == "compas.exec"));

    let list = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.catalog".into(),
            arguments: serde_json::json!({ "repo_root": repo_root_str, "view": "tools" })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.catalog tools");
    let list: ToolsListOutput = list.into_typed().expect("typed compas.catalog tools");
    assert!(
        list.ok,
        "compas.catalog tools ok=false; error={:?}",
        list.error
    );
    assert!(list.tools.iter().any(|t| t.id == "cargo-test"));
    assert!(
        list.tools
            .iter()
            .any(|t| t.id == "cargo-test" && t.plugin_id == "default")
    );
    assert!(list.tools.iter().all(|t| !t.description.trim().is_empty()));

    let plugins = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.catalog".into(),
            arguments: serde_json::json!({ "repo_root": repo_root_str, "view": "plugins" })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.catalog plugins");
    let plugins: serde_json::Value = plugins.into_typed().expect("typed compas.catalog plugins");
    assert_eq!(plugins.get("ok"), Some(&serde_json::Value::Bool(true)));
    assert!(
        plugins
            .get("plugins")
            .and_then(|v| v.as_array())
            .is_some_and(|arr| arr
                .iter()
                .any(|p| p.get("id") == Some(&serde_json::Value::String("default".to_string()))))
    );

    let plugin_details = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.catalog".into(),
            arguments: serde_json::json!({
                "repo_root": repo_root_str,
                "view": "plugin",
                "plugin_id": "default"
            })
            .as_object()
            .cloned(),
            task: None,
        })
        .await
        .expect("call compas.catalog plugin");
    let plugin_details: serde_json::Value = plugin_details
        .into_typed()
        .expect("typed compas.catalog plugin");
    assert_eq!(
        plugin_details.get("ok"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        plugin_details
            .get("plugin")
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str()),
        Some("default")
    );

    let validate = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.validate".into(),
            arguments: serde_json::json!({ "repo_root": repo_root_str, "mode": "warn" })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call validate");
    let validate: ValidateOutput = validate.into_typed().expect("typed validate");
    assert_eq!(validate.mode, ValidateMode::Warn);
    assert_eq!(validate.schema_version, "3");
    assert!(
        validate
            .summary_md
            .as_ref()
            .is_some_and(|s| s.contains("**Status:**")),
        "validate summary_md must be present"
    );
    assert_eq!(
        validate.payload_meta.as_ref().map(|m| m.mode),
        Some(ai_dx_mcp::api::ResponseMode::Compact)
    );
    let verdict = validate.verdict.as_ref().expect("verdict must be present");
    assert!(matches!(
        verdict.decision.status,
        ai_dx_mcp::api::DecisionStatus::Pass
            | ai_dx_mcp::api::DecisionStatus::Retryable
            | ai_dx_mcp::api::DecisionStatus::Blocked
    ));
    assert!(
        validate.quality_posture.is_some(),
        "quality_posture must be present in schema v3"
    );
    assert!(validate.boundary.is_some());
    assert!(validate.public_surface.is_some());

    // init is dry-run: plan only (no writes). Use a temp repo root to avoid touching this repo.
    let dir = tempfile::tempdir().expect("temp repo");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let tmp_root = dir.path().to_string_lossy().to_string();

    let init = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.init".into(),
            arguments: serde_json::json!({ "repo_root": tmp_root.clone(), "apply": false })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.init");
    let init: InitOutput = init.into_typed().expect("typed compas.init");
    assert!(init.ok, "init ok=false; error={:?}", init.error);
    assert!(!init.applied);
    let plan = init.plan.expect("plan");
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/plugins/default/plugin.toml")
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == ".agents/mcp/compas/packs/rust/pack.toml")
    );
    assert!(
        plan.writes
            .iter()
            .any(|w| w.path == "tools/custom/rust-test/tool.toml")
    );

    let init_apply = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.init".into(),
            arguments: serde_json::json!({ "repo_root": tmp_root, "apply": true })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.init apply");
    let init_apply: InitOutput = init_apply.into_typed().expect("typed compas.init apply");
    assert!(
        init_apply.ok,
        "init apply ok=false; error={:?}",
        init_apply.error
    );
    std::fs::write(dir.path().join("Cargo.lock"), "# lock").expect("write Cargo.lock");
    assert!(init_apply.applied);
    assert!(
        dir.path()
            .join(".agents/mcp/compas/plugins/default/plugin.toml")
            .is_file()
    );
    assert!(
        dir.path()
            .join("tools/custom/rust-test/tool.toml")
            .is_file()
    );
    assert!(dir.path().join(".agents/mcp/compas/packs.lock").is_file());

    client.close().await.ok();
    server.close().await.ok();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn mcp_env_defaults_repo_root_and_write_witness() {
    let _guard = env_lock().lock().expect("env lock");
    // SAFETY: env var mutation is global; we guard mutations with a process-wide lock in this test.
    unsafe { std::env::remove_var("AI_DX_REPO_ROOT") };
    unsafe { std::env::remove_var("AI_DX_WRITE_WITNESS") };

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move { AiDxServer::new().serve(server_io).await });
    let mut client = ().serve(client_io).await.expect("serve client");
    let mut server = server_task
        .await
        .expect("join server task")
        .expect("serve server");

    // Create a temp repo and bootstrap it via compas.init (apply=true).
    let dir = tempfile::tempdir().expect("temp repo");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let tmp_root = dir.path().to_string_lossy().to_string();

    let init_apply = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.init".into(),
            arguments: serde_json::json!({ "repo_root": tmp_root, "apply": true })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.init apply");
    let init_apply: InitOutput = init_apply.into_typed().expect("typed compas.init apply");
    assert!(
        init_apply.ok,
        "init apply ok=false; error={:?}",
        init_apply.error
    );
    std::fs::write(dir.path().join("Cargo.lock"), "# lock").expect("write Cargo.lock");

    let tmp_root = dir.path().to_string_lossy().to_string();
    // SAFETY: see remove_var above.
    unsafe { std::env::set_var("AI_DX_REPO_ROOT", &tmp_root) };

    // compas.catalog(view=tools) without repo_root should use AI_DX_REPO_ROOT.
    let list = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.catalog".into(),
            arguments: serde_json::json!({ "view": "tools" }).as_object().cloned(),
            task: None,
        })
        .await
        .expect("call compas.catalog");
    let list: ToolsListOutput = list.into_typed().expect("typed compas.catalog");
    assert!(list.ok, "compas.catalog ok=false; error={:?}", list.error);

    // compas.validate without repo_root should use AI_DX_REPO_ROOT.
    let validate = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.validate".into(),
            arguments: serde_json::json!({ "mode": "warn" }).as_object().cloned(),
            task: None,
        })
        .await
        .expect("call validate");
    let validate: ValidateOutput = validate.into_typed().expect("typed validate");
    assert_eq!(validate.mode, ValidateMode::Warn);
    assert_eq!(validate.schema_version, "3");
    assert!(validate.verdict.is_some(), "verdict must be present");
    assert!(
        validate.quality_posture.is_some(),
        "quality_posture must be present"
    );
    assert!(
        validate
            .summary_md
            .as_ref()
            .is_some_and(|s| s.contains("**Status:**"))
    );
    assert!(validate.ok, "validate ok=false; error={:?}", validate.error);

    // SAFETY: see remove_var above.
    unsafe { std::env::set_var("AI_DX_WRITE_WITNESS", "true") };

    // compas.gate without write_witness should honor AI_DX_WRITE_WITNESS.
    let gate = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.gate".into(),
            arguments: serde_json::json!({ "kind": "ci_fast", "dry_run": true })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call gate");
    let gate: serde_json::Value = gate.into_typed().expect("typed gate");
    assert_eq!(gate.get("ok"), Some(&serde_json::Value::Bool(true)));
    let gate_verdict_status = gate
        .get("verdict")
        .and_then(|v| v.get("decision"))
        .and_then(|d| d.get("status"))
        .and_then(|s| s.as_str())
        .expect("gate verdict.decision.status");
    assert!(
        matches!(gate_verdict_status, "pass" | "retryable" | "blocked"),
        "unexpected gate verdict status: {gate_verdict_status}"
    );
    let witness_path = gate
        .get("witness_path")
        .and_then(|v| v.as_str())
        .expect("witness_path");
    assert!(
        dir.path().join(witness_path).is_file(),
        "witness file missing: {witness_path}"
    );

    client.close().await.ok();
    server.close().await.ok();

    // SAFETY: see remove_var above.
    unsafe { std::env::remove_var("AI_DX_REPO_ROOT") };
    unsafe { std::env::remove_var("AI_DX_WRITE_WITNESS") };
}

#[tokio::test]
async fn mcp_gate_async_job_mode_status_roundtrip() {
    let dir = tempfile::tempdir().expect("temp repo");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let tmp_root = dir.path().to_string_lossy().to_string();

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move { AiDxServer::new().serve(server_io).await });
    let mut client = ().serve(client_io).await.expect("serve client");
    let mut server = server_task
        .await
        .expect("join server task")
        .expect("serve server");

    let init_apply = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.init".into(),
            arguments: serde_json::json!({ "repo_root": tmp_root, "apply": true })
                .as_object()
                .cloned(),
            task: None,
        })
        .await
        .expect("call compas.init apply");
    let init_apply: InitOutput = init_apply.into_typed().expect("typed compas.init apply");
    assert!(
        init_apply
            .summary_md
            .as_ref()
            .is_some_and(|s| s.contains("**Status:**"))
    );
    std::fs::write(dir.path().join("Cargo.lock"), "# lock").expect("write Cargo.lock");

    let start = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "compas.gate".into(),
            arguments: serde_json::json!({
                "repo_root": dir.path().to_string_lossy().to_string(),
                "kind": "ci_fast",
                "dry_run": true,
                "op": "start",
                "response_mode": "compact"
            })
            .as_object()
            .cloned(),
            task: None,
        })
        .await
        .expect("call gate start");
    let start: serde_json::Value = start.into_typed().expect("typed gate start");
    let job_id = start
        .get("job")
        .and_then(|j| j.get("job_id"))
        .and_then(|v| v.as_str())
        .expect("gate start job.job_id")
        .to_string();
    assert!(
        start
            .get("summary_md")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains("**Status:**"))
    );

    let mut final_status: Option<serde_json::Value> = None;
    for _ in 0..20 {
        let status = client
            .call_tool(CallToolRequestParams {
                meta: None,
                name: "compas.gate".into(),
                arguments: serde_json::json!({
                    "repo_root": dir.path().to_string_lossy().to_string(),
                    "kind": "ci_fast",
                    "op": "status",
                    "job_id": job_id,
                    "wait_ms": 500,
                    "response_mode": "compact"
                })
                .as_object()
                .cloned(),
                task: None,
            })
            .await
            .expect("call gate status");
        let status: serde_json::Value = status.into_typed().expect("typed gate status");
        let state = status
            .get("job_state")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if matches!(state, "succeeded" | "failed" | "expired") {
            final_status = Some(status);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let status = final_status.expect("gate status must reach terminal state");
    assert!(
        status
            .get("summary_md")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains("**Status:**"))
    );
    assert_eq!(
        status
            .get("payload_meta")
            .and_then(|m| m.get("mode"))
            .and_then(|v| v.as_str()),
        Some("compact")
    );

    client.close().await.ok();
    server.close().await.ok();
}
