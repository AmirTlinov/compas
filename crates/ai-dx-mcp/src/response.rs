use crate::{
    api::{
        DecisionStatus, GateJobState, GateOutput, InitOutput, PayloadMeta, ResponseMode,
        ToolsRunOutput, ValidateOutput,
    },
    server_catalog::CatalogOutput,
};
use std::collections::BTreeMap;

const DEFAULT_COMPACT_TOP_N: usize = 20;

fn compact_top_n() -> usize {
    std::env::var("AI_DX_COMPACT_TOP_N")
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_COMPACT_TOP_N)
}

fn truncate_vec<T>(
    key: &str,
    vec: &mut Vec<T>,
    top_n: usize,
    omitted: &mut BTreeMap<String, usize>,
) {
    if vec.len() > top_n {
        omitted.insert(key.to_string(), vec.len() - top_n);
        vec.truncate(top_n);
    }
}

fn compact_validate_payload(out: &mut ValidateOutput, top_n: usize) -> PayloadMeta {
    let mut omitted = BTreeMap::new();
    truncate_vec("violations", &mut out.violations, top_n, &mut omitted);
    truncate_vec("findings_v2", &mut out.findings_v2, top_n, &mut omitted);
    truncate_vec("suppressed", &mut out.suppressed, top_n, &mut omitted);
    if let Some(verdict) = out.verdict.as_mut() {
        truncate_vec(
            "verdict.suppressed_codes",
            &mut verdict.suppressed_codes,
            top_n,
            &mut omitted,
        );
    }

    PayloadMeta {
        mode: ResponseMode::Compact,
        truncated: !omitted.is_empty(),
        omitted,
    }
}

fn status_from_decision(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Pass => "pass",
        DecisionStatus::Retryable => "retryable",
        DecisionStatus::Blocked => "blocked",
    }
}

fn validate_summary(out: &ValidateOutput) -> String {
    let status = out
        .verdict
        .as_ref()
        .map(|v| status_from_decision(v.decision.status))
        .unwrap_or(if out.ok { "pass" } else { "blocked" });

    let why = if let Some(err) = &out.error {
        format!("{}.", err.code)
    } else {
        format!(
            "violations={}, suppressed={}.",
            out.violations.len(),
            out.suppressed.len()
        )
    };

    let next = match out.verdict.as_ref().map(|v| v.decision.status) {
        Some(DecisionStatus::Pass) if out.ok => "run compas.gate kind=ci_fast.",
        Some(DecisionStatus::Retryable) => "retry compas.gate after transient issue.",
        _ => {
            if let Some(top) = out.violations.first() {
                if !top.code.trim().is_empty() {
                    return format!(
                        "**Status:** {status}\n**Why:** {why}\n**Next:** fix `{}` and rerun compas.validate mode=ratchet.",
                        top.code
                    );
                }
            }
            "fix top violation and rerun compas.validate mode=ratchet."
        }
    };

    format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}")
}

fn gate_summary(out: &GateOutput) -> String {
    if let Some(state) = out.job_state {
        match state {
            GateJobState::Pending
            | GateJobState::Running
            | GateJobState::Failed
            | GateJobState::Expired => {
                let status = match state {
                    GateJobState::Pending => "pending",
                    GateJobState::Running => "running",
                    GateJobState::Failed => "failed",
                    GateJobState::Expired => "expired",
                    GateJobState::Succeeded => "succeeded",
                };
                let why = out
                    .job
                    .as_ref()
                    .map(|j| format!("job_id={}.", j.job_id))
                    .unwrap_or_else(|| "job tracking enabled.".to_string());
                let next = match state {
                    GateJobState::Pending | GateJobState::Running => {
                        if let Some(job) = &out.job {
                            format!("poll compas.gate op=status job_id={}.", job.job_id)
                        } else {
                            "poll compas.gate op=status with current job_id.".to_string()
                        }
                    }
                    GateJobState::Expired => {
                        "start a fresh gate job with compas.gate op=start.".to_string()
                    }
                    GateJobState::Failed => {
                        "inspect job_error and restart compas.gate op=start.".to_string()
                    }
                    GateJobState::Succeeded => "inspect final verdict and continue.".to_string(),
                };
                return format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}");
            }
            GateJobState::Succeeded => {}
        }
    }

    let status = out
        .verdict
        .as_ref()
        .map(|v| status_from_decision(v.decision.status))
        .unwrap_or(if out.ok { "pass" } else { "blocked" });

    let why = if let Some(err) = &out.error {
        format!("{}.", err.code)
    } else {
        format!("receipts={}.", out.receipts.len())
    };
    let next = match out.verdict.as_ref().map(|v| v.decision.status) {
        Some(DecisionStatus::Pass) if out.ok => "gate passed; proceed with delivery.",
        Some(DecisionStatus::Retryable) => "retry compas.gate (transient runner failure).",
        _ => "fix blocking findings and rerun compas.gate kind=ci_fast.",
    };
    format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}")
}

fn catalog_summary(out: &CatalogOutput) -> String {
    let status = if out.ok { "ok" } else { "error" };
    let plugins = out.plugins.as_ref().map_or(0, Vec::len);
    let tools = out.tools.as_ref().map_or(0, Vec::len);
    let why = if let Some(err) = &out.error {
        format!("{}.", err.code)
    } else if out.tool.is_some() {
        "single tool details returned.".to_string()
    } else if out.plugin.is_some() {
        "single plugin details returned.".to_string()
    } else {
        format!("plugins={}, tools={}.", plugins, tools)
    };
    let next = if out.ok {
        "use compas.exec tool_id=<id> or compas.gate kind=ci_fast.".to_string()
    } else {
        "fix config error and rerun compas.catalog.".to_string()
    };
    format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}")
}

fn exec_summary(out: &ToolsRunOutput) -> String {
    let status = if out.ok { "ok" } else { "error" };
    let why = if let Some(err) = &out.error {
        format!("{}.", err.code)
    } else if let Some(r) = &out.receipt {
        format!("tool_id={}, duration_ms={}.", r.tool_id, r.duration_ms)
    } else {
        "receipt missing.".to_string()
    };
    let next = if out.ok {
        "use compas.gate kind=ci_fast for integrated check.".to_string()
    } else {
        "inspect stderr_tail and rerun compas.exec.".to_string()
    };
    format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}")
}

fn init_summary(out: &InitOutput) -> String {
    let status = if out.ok {
        if out.applied { "applied" } else { "planned" }
    } else {
        "error"
    };
    let (writes, deletes) = out
        .plan
        .as_ref()
        .map(|p| (p.writes.len(), p.deletes.len()))
        .unwrap_or((0, 0));
    let why = if let Some(err) = &out.error {
        format!("{}.", err.code)
    } else {
        format!("writes={}, deletes={}.", writes, deletes)
    };
    let next = if out.ok && out.applied {
        "run compas.validate mode=ratchet.".to_string()
    } else if out.ok {
        "review plan and rerun compas.init apply=true.".to_string()
    } else {
        "fix init error and rerun compas.init.".to_string()
    };
    format!("**Status:** {status}\n**Why:** {why}\n**Next:** {next}")
}

pub fn finalize_validate(mut out: ValidateOutput, mode: ResponseMode) -> ValidateOutput {
    out.payload_meta = match mode {
        ResponseMode::Compact => Some(compact_validate_payload(&mut out, compact_top_n())),
        ResponseMode::Full => None,
    };
    out.summary_md = Some(validate_summary(&out));
    out
}

pub fn finalize_gate(mut out: GateOutput, mode: ResponseMode) -> GateOutput {
    let has_final_payload = out.verdict.is_some()
        || !out.receipts.is_empty()
        || out.witness_path.is_some()
        || !out.validate.violations.is_empty()
        || out.validate.verdict.is_some();

    if matches!(mode, ResponseMode::Compact) {
        let mut omitted = BTreeMap::new();
        let top_n = compact_top_n();
        truncate_vec("receipts", &mut out.receipts, top_n, &mut omitted);
        if has_final_payload {
            out.validate = finalize_validate(out.validate, ResponseMode::Compact);
        }
        out.payload_meta = Some(PayloadMeta {
            mode: ResponseMode::Compact,
            truncated: !omitted.is_empty(),
            omitted,
        });
    } else {
        out.payload_meta = None;
        if has_final_payload {
            out.validate = finalize_validate(out.validate, ResponseMode::Full);
        }
    }
    out.summary_md = Some(gate_summary(&out));
    out
}

pub(crate) fn finalize_catalog(mut out: CatalogOutput, mode: ResponseMode) -> CatalogOutput {
    out.payload_meta = match mode {
        ResponseMode::Compact => {
            let top_n = compact_top_n();
            let mut omitted = BTreeMap::new();
            if let Some(v) = out.plugins.as_mut() {
                truncate_vec("plugins", v, top_n, &mut omitted);
            }
            if let Some(v) = out.tools.as_mut() {
                truncate_vec("tools", v, top_n, &mut omitted);
            }
            if let Some(plugin) = out.plugin.as_mut() {
                truncate_vec("plugin.tools", &mut plugin.tools, top_n, &mut omitted);
            }
            Some(PayloadMeta {
                mode: ResponseMode::Compact,
                truncated: !omitted.is_empty(),
                omitted,
            })
        }
        ResponseMode::Full => None,
    };
    out.summary_md = Some(catalog_summary(&out));
    out
}

pub fn finalize_exec(mut out: ToolsRunOutput) -> ToolsRunOutput {
    out.summary_md = Some(exec_summary(&out));
    out
}

pub fn finalize_init(mut out: InitOutput) -> InitOutput {
    out.summary_md = Some(init_summary(&out));
    out
}
