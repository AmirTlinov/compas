use crate::api::*;
use crate::server_catalog::{CatalogOutput, CatalogRequest, catalog, exec};
use rmcp::{
    Json, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
};

#[derive(Clone)]
pub struct AiDxServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AiDxServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_repo_root(repo_root: &Option<String>) -> String {
        repo_root
            .clone()
            .or_else(|| std::env::var("AI_DX_REPO_ROOT").ok())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| ".".to_string())
    }

    fn resolve_gate_call_budget_ms() -> Option<u64> {
        const DEFAULT_GATE_BUDGET_MS: u64 = 55_000;
        match std::env::var("AI_DX_GATE_CALL_BUDGET_MS") {
            Ok(v) => match v.trim().parse::<u64>() {
                Ok(0) => None,
                Ok(ms) => Some(ms),
                Err(_) => Some(DEFAULT_GATE_BUDGET_MS),
            },
            Err(_) => Some(DEFAULT_GATE_BUDGET_MS),
        }
    }

    #[tool(
        name = "compas.validate",
        description = "Validate repo: checks + config (ratchet/strict/warn). Fail-closed; default is ratchet."
    )]
    async fn validate(&self, params: Parameters<ValidateRequest>) -> Json<ValidateOutput> {
        let repo_root = Self::resolve_repo_root(&params.0.repo_root);
        let write_baseline = params.0.write_baseline.unwrap_or(false);
        let response_mode = params.0.response_mode.unwrap_or(ResponseMode::Compact);
        Json(crate::response::finalize_validate(
            crate::app::validate(
                &repo_root,
                params.0.mode,
                write_baseline,
                params.0.baseline_maintenance.as_ref(),
            ),
            response_mode,
        ))
    }

    #[tool(
        name = "compas.init",
        description = "Bootstrap compas config via language packs; apply=true writes files (conflicts fail-closed)."
    )]
    async fn compas_init(&self, params: Parameters<InitRequest>) -> Json<InitOutput> {
        let repo_root = Self::resolve_repo_root(&params.0.repo_root);
        Json(crate::response::finalize_init(crate::init::init(
            &repo_root, params.0,
        )))
    }

    #[tool(
        name = "compas.catalog",
        description = "Catalog browser for plugins/tools (view=all|plugins|plugin|tools|tool)."
    )]
    async fn compas_catalog(&self, params: Parameters<CatalogRequest>) -> Json<CatalogOutput> {
        let repo_root = Self::resolve_repo_root(&params.0.repo_root);
        let response_mode = params.0.response_mode.unwrap_or(ResponseMode::Compact);
        Json(crate::response::finalize_catalog(
            catalog(&repo_root, &params.0),
            response_mode,
        ))
    }

    #[tool(
        name = "compas.exec",
        description = "Run tool_id with optional extra args (no shell). Returns receipt with bounded stdout/stderr tails."
    )]
    async fn compas_exec(&self, params: Parameters<ToolsRunRequest>) -> Json<ToolsRunOutput> {
        let repo_root = Self::resolve_repo_root(&params.0.repo_root);
        Json(crate::response::finalize_exec(
            exec(&repo_root, &params.0).await,
        ))
    }

    #[tool(
        name = "compas.gate",
        description = "Run gate kind=ci_fast|ci|flagship: validate + wired toolchain; op=run|start|status enables stable long-run jobs."
    )]
    async fn gate(&self, params: Parameters<GateRequest>) -> Json<GateOutput> {
        let repo_root = Self::resolve_repo_root(&params.0.repo_root);
        let dry_run = params.0.dry_run.unwrap_or(false);
        let response_mode = params.0.response_mode.unwrap_or(ResponseMode::Compact);
        let op = params.0.op.unwrap_or(GateOp::Run);
        let write_witness = params.0.write_witness.unwrap_or_else(|| {
            std::env::var("AI_DX_WRITE_WITNESS")
                .ok()
                .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        });
        if let Err(err) =
            crate::gate_jobs::validate_gate_status_args(op, params.0.job_id.as_deref())
        {
            return Json(crate::response::finalize_gate(
                GateOutput {
                    ok: false,
                    error: Some(err),
                    repo_root: repo_root.clone(),
                    kind: params.0.kind,
                    validate: crate::gate_jobs::empty_validate_output(&repo_root),
                    receipts: vec![],
                    witness_path: None,
                    witness: None,
                    verdict: None,
                    agent_digest: None,
                    summary_md: None,
                    payload_meta: None,
                    job: None,
                    job_state: None,
                    job_error: None,
                },
                response_mode,
            ));
        }

        let raw = match op {
            GateOp::Run => {
                crate::app::gate_with_budget(
                    &repo_root,
                    params.0.kind,
                    dry_run,
                    write_witness,
                    Self::resolve_gate_call_budget_ms(),
                )
                .await
            }
            GateOp::Start => {
                crate::gate_jobs::start(
                    &repo_root,
                    params.0.kind,
                    dry_run,
                    write_witness,
                    Self::resolve_gate_call_budget_ms(),
                )
                .await
            }
            GateOp::Status => {
                crate::gate_jobs::status(
                    &repo_root,
                    params.0.kind,
                    params.0.job_id.as_deref().unwrap_or_default(),
                    params.0.wait_ms,
                )
                .await
            }
        };

        Json(crate::response::finalize_gate(raw, response_mode))
    }
}

impl Default for AiDxServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler]
impl ServerHandler for AiDxServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "compas v2 (agent-first DX): lean core surface.\n\nQuickstart:\n  1) If missing `.agents/mcp/compas/plugins`, run `compas.init` (apply=true).\n  2) Run `compas.gate` kind=ci_fast (dry_run=true for preview).\n  3) Discover plugin/tool wiring via `compas.catalog`.\n\nEnv defaults:\n  - AI_DX_REPO_ROOT=<path>\n  - AI_DX_WRITE_WITNESS=1|true\n"
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
