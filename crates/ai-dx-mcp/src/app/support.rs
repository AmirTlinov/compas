use crate::{
    api::{ApiError, ValidateMode, ValidateOutput, Violation},
    repo::RepoConfig,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn empty_output_with_error(
    repo_root: &str,
    mode: ValidateMode,
    error: ApiError,
    verdict: Option<crate::api::Verdict>,
) -> ValidateOutput {
    ValidateOutput {
        ok: false,
        error: Some(error),
        schema_version: "3".to_string(),
        repo_root: repo_root.to_string(),
        mode,
        violations: vec![],
        findings_v2: vec![],
        suppressed: vec![],
        loc: None,
        boundary: None,
        public_surface: None,
        effective_config: None,
        risk_summary: None,
        coverage: None,
        trust_score: None,
        verdict,
        quality_posture: None,
        agent_digest: None,
        summary_md: None,
        payload_meta: None,
    }
}

pub(super) fn compute_checks_hash(cfg: &RepoConfig) -> String {
    let canonical = serde_json::to_string(&cfg.checks).unwrap_or_default();
    format!("sha256:{}", crate::hash::sha256_hex(canonical.as_bytes()))
}

pub(super) fn has_prior_baselines(repo_root: &Path) -> bool {
    let base = repo_root.join(".agents/mcp/compas/baselines");
    base.join("loc.json").is_file()
        || base.join("public_surface.json").is_file()
        || base.join("duplicates.json").is_file()
}

pub(super) fn collect_suppressed_codes(violations: &[Violation]) -> Vec<String> {
    violations
        .iter()
        .map(|v| v.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn detect_tool_duplicates(cfg: &RepoConfig) -> Vec<Violation> {
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Signature {
        command: String,
        args: Vec<String>,
        cwd: Option<String>,
        timeout_ms: Option<u64>,
        max_stdout_bytes: Option<usize>,
        max_stderr_bytes: Option<usize>,
        env_pairs: Vec<(String, String)>,
    }

    let mut by_signature: BTreeMap<Signature, Vec<String>> = BTreeMap::new();
    for (tool_id, tool) in &cfg.tools {
        let mut env_pairs = tool
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        env_pairs.sort();
        let sig = Signature {
            command: tool.command.trim().to_ascii_lowercase(),
            args: tool.args.clone(),
            cwd: tool.cwd.clone(),
            timeout_ms: tool.timeout_ms,
            max_stdout_bytes: tool.max_stdout_bytes,
            max_stderr_bytes: tool.max_stderr_bytes,
            env_pairs,
        };
        by_signature.entry(sig).or_default().push(tool_id.clone());
    }

    let mut violations: Vec<Violation> = vec![];
    let mut exact_colliders: BTreeSet<String> = BTreeSet::new();

    for (sig, tools) in by_signature {
        if tools.len() > 1 {
            for t in &tools {
                exact_colliders.insert(t.clone());
            }
            violations.push(Violation::blocking(
                "tools.duplicate_exact",
                format!(
                    "exact duplicate tool signature detected for {} tools",
                    tools.len()
                ),
                None,
                Some(serde_json::json!({
                    "tools": tools,
                    "command": sig.command,
                    "args": sig.args,
                    "cwd": sig.cwd,
                    "timeout_ms": sig.timeout_ms,
                    "max_stdout_bytes": sig.max_stdout_bytes,
                    "max_stderr_bytes": sig.max_stderr_bytes,
                })),
            ));
        }
    }

    // Conservative semantic signal (observation only):
    // same command + identical normalized description, but not exact signature duplicates.
    let mut semantic_groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (tool_id, tool) in &cfg.tools {
        if exact_colliders.contains(tool_id) {
            continue;
        }
        let key = (
            tool.command.trim().to_ascii_lowercase(),
            tool.description.trim().to_ascii_lowercase(),
        );
        semantic_groups
            .entry(key)
            .or_default()
            .push(tool_id.clone());
    }
    for ((command, description), tools) in semantic_groups {
        if tools.len() > 1 {
            violations.push(Violation::observation(
                "tools.duplicate_semantic",
                format!(
                    "semantically similar tools detected (same command+description): {}",
                    tools.len()
                ),
                None,
                Some(serde_json::json!({
                    "tools": tools,
                    "command": command,
                    "description": description,
                })),
            ));
        }
    }

    violations
}
