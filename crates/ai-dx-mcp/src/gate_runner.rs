use crate::{
    api::{ApiError, GateKind, GateOutput, Receipt, ValidateMode, Violation},
    app::{map_config_error, validate},
    config::{ImpactUnmappedPathPolicy, QualityContractConfig, ToolReceiptContract},
    repo::load_repo_config,
    runner::run_project_tool_with_timeout_override,
    structured_report::ingest_tool_report,
    validate_insights::build_agent_digest,
    witness::maybe_write_gate_witness,
};
use globset::{Glob, GlobSetBuilder};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn gate_fail(
    repo_root: &str,
    kind: GateKind,
    validate: crate::api::ValidateOutput,
    receipts: Vec<Receipt>,
    mut receipt_violations: Vec<Violation>,
    error: ApiError,
) -> GateOutput {
    receipt_violations.push(Violation::blocking(
        error.code.clone(),
        error.message.clone(),
        None,
        None,
    ));
    let verdict = Some(crate::judge::judge_gate(
        &validate.violations,
        &receipt_violations,
        &receipts,
    ));
    let digest = verdict
        .as_ref()
        .map(|v| build_agent_digest(&v.decision, &receipt_violations, &validate.findings_v2));
    GateOutput {
        ok: false,
        error: Some(error),
        repo_root: repo_root.to_string(),
        kind,
        validate,
        receipts,
        witness_path: None,
        witness: None,
        verdict,
        agent_digest: digest,
        summary_md: None,
        payload_meta: None,
        job: None,
        job_state: None,
        job_error: None,
    }
}

fn ensure_gate_sequence_invariants(kind: GateKind, tool_ids: &[String]) -> Result<(), ApiError> {
    if tool_ids.is_empty() {
        return Err(ApiError {
            code: "gate.empty_sequence".to_string(),
            message: format!("gate kind={kind:?} has empty tool sequence"),
        });
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for tool_id in tool_ids {
        let id = tool_id.as_str();
        if !seen.insert(id) {
            return Err(ApiError {
                code: "gate.duplicate_tool_id".to_string(),
                message: format!("gate kind={kind:?} contains duplicate tool_id={id}"),
            });
        }
    }
    Ok(())
}

fn ensure_receipt_invariants(r: &Receipt) -> Result<(), ApiError> {
    if !r.success && r.exit_code.is_none() && !r.timed_out {
        return Err(ApiError {
            code: "gate.receipt_invariant_failed".to_string(),
            message: format!(
                "tool receipt missing failure context: tool_id={}",
                r.tool_id
            ),
        });
    }

    if r.stdout_sha256.trim().is_empty() || r.stderr_sha256.trim().is_empty() {
        return Err(ApiError {
            code: "gate.receipt_invariant_failed".to_string(),
            message: format!("tool receipt missing stream hash: tool_id={}", r.tool_id),
        });
    }

    Ok(())
}

fn check_receipt_contract(
    receipt: &Receipt,
    contract: &ToolReceiptContract,
) -> Result<(), Violation> {
    if let Some(min_duration_ms) = contract.min_duration_ms
        && receipt.duration_ms < min_duration_ms
    {
        return Err(Violation::blocking(
            "gate.receipt_contract_violated",
            format!(
                "tool {} ran too fast: {}ms < min {}ms",
                receipt.tool_id, receipt.duration_ms, min_duration_ms
            ),
            None,
            None,
        ));
    }
    if let Some(min_stdout_bytes) = contract.min_stdout_bytes
        && receipt.stdout_bytes < min_stdout_bytes
    {
        return Err(Violation::blocking(
            "gate.receipt_contract_violated",
            format!(
                "tool {} produced too little output: {} bytes < min {} bytes",
                receipt.tool_id, receipt.stdout_bytes, min_stdout_bytes
            ),
            None,
            None,
        ));
    }
    if let Some(pattern) = contract
        .expect_stdout_pattern
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let re = Regex::new(pattern).map_err(|e| {
            Violation::blocking(
                "gate.receipt_contract_violated",
                format!(
                    "invalid expect_stdout_pattern regex for {}: {e}",
                    receipt.tool_id
                ),
                None,
                None,
            )
        })?;
        let combined_tail = if receipt.stdout_tail.is_empty() {
            receipt.stderr_tail.clone()
        } else if receipt.stderr_tail.is_empty() {
            receipt.stdout_tail.clone()
        } else {
            format!("{}\n{}", receipt.stdout_tail, receipt.stderr_tail)
        };
        let matches = re.is_match(&receipt.stdout_tail)
            || re.is_match(&receipt.stderr_tail)
            || re.is_match(&combined_tail);
        if !matches {
            return Err(Violation::blocking(
                "gate.receipt_contract_violated",
                format!(
                    "tool {} output tails do not match expected pattern {:?} \
                    (checked stdout_tail+stderr_tail; stdout_tail_len_bytes={}, stderr_tail_len_bytes={}, stdout_bytes={}, stderr_bytes={})",
                    receipt.tool_id,
                    pattern,
                    receipt.stdout_tail.len(),
                    receipt.stderr_tail.len(),
                    receipt.stdout_bytes,
                    receipt.stderr_bytes
                ),
                None,
                None,
            ));
        }
    }
    if let Some(expect_codes) = &contract.expect_exit_codes
        && !expect_codes.is_empty()
    {
        let got = receipt.exit_code.unwrap_or(-9999);
        if !expect_codes.contains(&got) {
            return Err(Violation::blocking(
                "gate.receipt_contract_violated",
                format!(
                    "tool {} exit code {:?} not in expected {:?}",
                    receipt.tool_id, receipt.exit_code, expect_codes
                ),
                None,
                None,
            ));
        }
    }
    Ok(())
}

fn effective_receipt_contract(
    tool_contract: Option<&ToolReceiptContract>,
    quality_contract: Option<&QualityContractConfig>,
) -> Option<ToolReceiptContract> {
    if let Some(c) = tool_contract {
        return Some(c.clone());
    }
    quality_contract.map(|qc| ToolReceiptContract {
        min_duration_ms: Some(qc.receipt_defaults.min_duration_ms),
        min_stdout_bytes: Some(qc.receipt_defaults.min_stdout_bytes),
        expect_stdout_pattern: None,
        expect_exit_codes: None,
    })
}

fn classify_run_failed(err: &std::io::Error) -> &'static str {
    match err.kind() {
        std::io::ErrorKind::TimedOut
        | std::io::ErrorKind::Interrupted
        | std::io::ErrorKind::WouldBlock
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::BrokenPipe => "gate.run_failed_transient",
        _ => "gate.run_failed",
    }
}

fn remaining_budget_ms(started_at: Instant, total_ms: u64) -> u64 {
    total_ms.saturating_sub(started_at.elapsed().as_millis() as u64)
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git {:?}: {e}", args))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {:?} failed: {}", args, err));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_diff_base(repo_root: &Path, diff_base: &str) -> Result<String, String> {
    if let Some(target) = diff_base.strip_prefix("merge-base:") {
        let target = target.trim();
        let mut candidates: Vec<String> = vec![];
        if target.eq_ignore_ascii_case("auto") {
            candidates.extend(
                ["origin/main", "origin/master", "main", "master"]
                    .iter()
                    .map(|s| s.to_string()),
            );
        } else {
            candidates.push(target.to_string());
            if target == "origin/main" {
                candidates.extend(
                    ["origin/master", "main", "master"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            } else if target == "origin/master" {
                candidates.extend(
                    ["origin/main", "main", "master"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            }
        }
        for candidate in candidates {
            if let Ok(base) = run_git(repo_root, &["merge-base", "HEAD", candidate.as_str()]) {
                return Ok(base);
            }
        }
        if run_git(repo_root, &["rev-parse", "--verify", "HEAD~1"]).is_ok() {
            return Ok("HEAD~1".to_string());
        }
        if run_git(repo_root, &["rev-parse", "--verify", "HEAD"]).is_ok() {
            return Ok("HEAD".to_string());
        }
        Err(format!(
            "unable to resolve merge-base target '{}' for change_impact",
            target
        ))
    } else {
        Ok(diff_base.to_string())
    }
}

fn collect_changed_files(repo_root: &Path, diff_base: &str) -> Result<Vec<String>, String> {
    let base = resolve_diff_base(repo_root, diff_base)?;
    let out = run_git(
        repo_root,
        &["diff", "--name-only", &format!("{base}...HEAD")],
    )?;
    let mut files = out
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Ok(files)
}

fn required_tools_for_changes(
    contract: &QualityContractConfig,
    changed_files: &[String],
) -> Result<(BTreeSet<String>, Vec<String>), String> {
    let mut required: BTreeSet<String> = BTreeSet::new();
    let mut unmatched: Vec<String> = vec![];
    for path in changed_files {
        let mut matched = false;
        for rule in &contract.impact.rules {
            if rule.path_globs.is_empty() || rule.required_tools.is_empty() {
                continue;
            }
            let mut b = GlobSetBuilder::new();
            for p in &rule.path_globs {
                let g =
                    Glob::new(p).map_err(|e| format!("invalid change_impact glob {:?}: {e}", p))?;
                b.add(g);
            }
            let set = b
                .build()
                .map_err(|e| format!("failed to build change_impact globset: {e}"))?;
            if set.is_match(path) {
                matched = true;
                for t in &rule.required_tools {
                    required.insert(t.clone());
                }
            }
        }
        if !matched {
            unmatched.push(path.clone());
        }
    }
    Ok((required, unmatched))
}

fn unmapped_path_violations(
    policy: ImpactUnmappedPathPolicy,
    unmatched: &[String],
) -> Vec<Violation> {
    let mut out = vec![];
    for path in unmatched {
        let message = format!(
            "changed path '{}' is not mapped in quality_contract [impact].rules",
            path
        );
        match policy {
            ImpactUnmappedPathPolicy::Block => out.push(Violation::blocking(
                "change_impact.unmapped_path",
                message,
                Some(path.clone()),
                None,
            )),
            ImpactUnmappedPathPolicy::Observe => out.push(Violation::observation(
                "change_impact.unmapped_path",
                message,
                Some(path.clone()),
                None,
            )),
            ImpactUnmappedPathPolicy::Ignore => {}
        }
    }
    out
}

pub(crate) async fn gate(
    repo_root: &str,
    kind: GateKind,
    dry_run: bool,
    write_witness: bool,
    gate_budget_ms: Option<u64>,
) -> GateOutput {
    let gate_started_at = Instant::now();

    // Always validate in ratchet mode first (fail-closed).
    let validate = validate(repo_root, ValidateMode::Ratchet, false, None);
    let mut receipt_violations: Vec<Violation> = vec![];

    if !validate.ok {
        let out = gate_fail(
            repo_root,
            kind,
            validate,
            vec![],
            receipt_violations,
            ApiError {
                code: "gate.validate_failed".to_string(),
                message: "validate(ratchet) failed; gate aborted".to_string(),
            },
        );
        return maybe_write_gate_witness(Path::new(repo_root), kind, write_witness, out);
    }

    let cfg = match load_repo_config(Path::new(repo_root)) {
        Ok(c) => c,
        Err(e) => {
            let out = gate_fail(
                repo_root,
                kind,
                validate,
                vec![],
                receipt_violations,
                map_config_error(repo_root, e),
            );
            return maybe_write_gate_witness(Path::new(repo_root), kind, write_witness, out);
        }
    };

    let tool_ids: Vec<String> = match kind {
        GateKind::CiFast => cfg.gate.ci_fast.clone(),
        GateKind::Ci => cfg.gate.ci.clone(),
        GateKind::Flagship => cfg.gate.flagship.clone(),
    };
    if let Err(err) = ensure_gate_sequence_invariants(kind, &tool_ids) {
        let out = gate_fail(repo_root, kind, validate, vec![], receipt_violations, err);
        return maybe_write_gate_witness(Path::new(repo_root), kind, write_witness, out);
    }

    if let Some(contract) = &cfg.quality_contract
        && !contract.impact.rules.is_empty()
    {
        match collect_changed_files(Path::new(repo_root), &contract.impact.diff_base) {
            Ok(changed) => match required_tools_for_changes(contract, &changed) {
                Ok((required_tools, unmatched)) => {
                    let selected: BTreeSet<String> = tool_ids.iter().cloned().collect();
                    for required in required_tools {
                        if !selected.contains(&required) {
                            receipt_violations.push(Violation::blocking(
                                "change_impact.required_tool_missing",
                                format!(
                                    "changed files require tool '{}', but it is not in selected gate {:?}",
                                    required, kind
                                ),
                                None,
                                None,
                            ));
                        }
                    }
                    receipt_violations.extend(unmapped_path_violations(
                        contract.impact.unmapped_path_policy,
                        &unmatched,
                    ));
                }
                Err(msg) => receipt_violations.push(Violation::blocking(
                    "change_impact.check_failed",
                    msg,
                    None,
                    None,
                )),
            },
            Err(msg) => receipt_violations.push(Violation::blocking(
                "change_impact.diff_failed",
                msg,
                None,
                None,
            )),
        }
    }

    let mut receipts: Vec<Receipt> = vec![];
    for tool_id in tool_ids {
        if let Some(total_ms) = gate_budget_ms
            && remaining_budget_ms(gate_started_at, total_ms) == 0
        {
            receipt_violations.push(Violation::blocking(
                "gate.run_failed_transient",
                format!("gate call budget exhausted before tool_id={tool_id}"),
                None,
                Some(serde_json::json!({ "budget_ms": total_ms })),
            ));
            break;
        }

        let tool = match cfg.tools.get(&tool_id) {
            Some(t) => t,
            None => {
                let out = gate_fail(
                    repo_root,
                    kind,
                    validate,
                    receipts,
                    receipt_violations,
                    ApiError {
                        code: "gate.unknown_tool_id".to_string(),
                        message: format!("gate references unknown tool_id={tool_id}"),
                    },
                );
                return maybe_write_gate_witness(Path::new(repo_root), kind, write_witness, out);
            }
        };

        let timeout_override_ms =
            gate_budget_ms.map(|total_ms| remaining_budget_ms(gate_started_at, total_ms));

        match run_project_tool_with_timeout_override(
            Path::new(repo_root),
            tool,
            &[],
            dry_run,
            timeout_override_ms,
        )
        .await
        {
            Ok(mut r) => {
                if let Err(err) = ensure_receipt_invariants(&r) {
                    let out =
                        gate_fail(repo_root, kind, validate, receipts, receipt_violations, err);
                    return maybe_write_gate_witness(
                        Path::new(repo_root),
                        kind,
                        write_witness,
                        out,
                    );
                }
                if !dry_run
                    && r.success
                    && let Some(contract) = effective_receipt_contract(
                        tool.receipt_contract.as_ref(),
                        cfg.quality_contract.as_ref(),
                    )
                    && let Err(v) = check_receipt_contract(&r, &contract)
                {
                    receipt_violations.push(v);
                }
                if !dry_run && let Some(report_cfg) = &tool.report {
                    let (report, mut violations) =
                        ingest_tool_report(Path::new(repo_root), &tool.id, report_cfg);
                    r.structured_report = report;
                    receipt_violations.append(&mut violations);
                }

                let success = r.success;
                receipts.push(r);
                if !success {
                    break;
                }
            }
            Err(e) => {
                receipt_violations.push(Violation::blocking(
                    classify_run_failed(&e),
                    format!("tool_id={tool_id}: {e}"),
                    None,
                    None,
                ));
                break;
            }
        }
    }

    let verdict = crate::judge::judge_gate(&validate.violations, &receipt_violations, &receipts);
    let ok = matches!(verdict.decision.status, crate::api::DecisionStatus::Pass);
    let error = match verdict.decision.status {
        crate::api::DecisionStatus::Pass => None,
        crate::api::DecisionStatus::Retryable => Some(ApiError {
            code: "gate.retryable".to_string(),
            message: "gate failed due to transient runner/tool timeout issue; retry is allowed"
                .to_string(),
        }),
        crate::api::DecisionStatus::Blocked => Some(ApiError {
            code: "gate.blocked".to_string(),
            message: "gate blocked by policy/quality violations".to_string(),
        }),
    };

    let effective_write_witness = if dry_run {
        write_witness
    } else if let Some(contract) = &cfg.quality_contract {
        write_witness || contract.proof.require_witness
    } else {
        write_witness
    };

    let gate_digest = build_agent_digest(
        &verdict.decision,
        &receipt_violations,
        &validate.findings_v2,
    );
    let out = GateOutput {
        ok,
        error,
        repo_root: repo_root.to_string(),
        kind,
        validate,
        receipts,
        witness_path: None,
        witness: None,
        verdict: Some(verdict),
        agent_digest: Some(gate_digest),
        summary_md: None,
        payload_meta: None,
        job: None,
        job_state: None,
        job_error: None,
    };
    maybe_write_gate_witness(Path::new(repo_root), kind, effective_write_witness, out)
}

#[cfg(test)]
mod tests;
