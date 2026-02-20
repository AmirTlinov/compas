use ai_dx_mcp::{
    api::{BaselineMaintenance, GateKind, InitRequest, ResponseMode, ValidateMode},
    response::{finalize_gate, finalize_init, finalize_validate},
    server::AiDxServer,
};
use rmcp::ServiceExt;
mod mcp_stdio;

fn print_version() {
    println!("{}", env!("CARGO_PKG_VERSION"));
}

fn print_help() {
    println!(
        "Usage:\n  compas_mcp help\n  compas_mcp version\n  compas_mcp init [--apply] [--packs <builtin:...,...>] [--repo-root <path>]\n  compas_mcp validate [ratchet|strict|warn] [--write-baseline] [--baseline-reason <text>] [--baseline-owner <id>] [--repo-root <path>]\n  compas_mcp gate [ci-fast|ci|flagship] [--dry-run] [--write-witness] [--repo-root <path>]\n\nNotes:\n  - No args => start MCP server over stdio.\n  - v1-style flags --init/--validate/--gate are removed in v2.\n  - Defaults via env:\n      AI_DX_REPO_ROOT=<path>\n      AI_DX_WRITE_WITNESS=1|true\n\nExamples:\n  compas_mcp init --apply\n  compas_mcp validate ratchet\n  compas_mcp validate ratchet --write-baseline --baseline-reason \"Quarterly baseline refresh after policy change\" --baseline-owner team-lead\n  compas_mcp gate ci-fast --dry-run\n"
    );
}

fn default_repo_root(repo_root: Option<String>) -> String {
    repo_root
        .or_else(|| std::env::var("AI_DX_REPO_ROOT").ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| ".".to_string())
}

fn parse_validate_mode(s: &str) -> Option<ValidateMode> {
    match s {
        "ratchet" => Some(ValidateMode::Ratchet),
        "strict" => Some(ValidateMode::Strict),
        "warn" => Some(ValidateMode::Warn),
        _ => None,
    }
}

fn parse_gate_kind(s: &str) -> Option<GateKind> {
    match s {
        "ci-fast" | "ci_fast" => Some(GateKind::CiFast),
        "ci" => Some(GateKind::Ci),
        "flagship" => Some(GateKind::Flagship),
        _ => None,
    }
}

fn is_v1_flag(arg: &str) -> bool {
    matches!(arg, "--init" | "--validate" | "--gate")
}

fn parse_validate_cli(
    args: &[String],
) -> Result<(ValidateMode, bool, String, Option<BaselineMaintenance>), String> {
    let mut mode = ValidateMode::Ratchet;
    let mut mode_set = false;
    let mut write_baseline = false;
    let mut repo_root: Option<String> = None;
    let mut baseline_reason: Option<String> = None;
    let mut baseline_owner: Option<String> = None;

    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--write-baseline" => {
                write_baseline = true;
                i += 1;
            }
            "--repo-root" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--repo-root requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--repo-root requires a value".to_string());
                }
                repo_root = Some(v.clone());
                i += 2;
            }
            "--baseline-reason" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--baseline-reason requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--baseline-reason requires a value".to_string());
                }
                baseline_reason = Some(v.clone());
                i += 2;
            }
            "--baseline-owner" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--baseline-owner requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--baseline-owner requires a value".to_string());
                }
                baseline_owner = Some(v.clone());
                i += 2;
            }
            _ if !a.starts_with("--") && !mode_set => {
                mode =
                    parse_validate_mode(a).ok_or_else(|| format!("unknown validate mode: {a}"))?;
                mode_set = true;
                i += 1;
            }
            _ => return Err(format!("unknown argument: {a}")),
        }
    }

    let baseline_maintenance = match (baseline_reason, baseline_owner) {
        (None, None) => None,
        (Some(reason), Some(owner)) => Some(BaselineMaintenance { reason, owner }),
        (Some(_), None) => {
            return Err(
                "--baseline-owner is required when --baseline-reason is provided".to_string(),
            );
        }
        (None, Some(_)) => {
            return Err(
                "--baseline-reason is required when --baseline-owner is provided".to_string(),
            );
        }
    };

    Ok((
        mode,
        write_baseline,
        default_repo_root(repo_root),
        baseline_maintenance,
    ))
}

fn parse_gate_cli(args: &[String]) -> Result<(GateKind, bool, bool, String), String> {
    let mut kind = GateKind::CiFast;
    let mut kind_set = false;
    let mut dry_run = false;
    let mut write_witness = false;
    let mut repo_root: Option<String> = None;

    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            "--write-witness" => {
                write_witness = true;
                i += 1;
            }
            "--repo-root" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--repo-root requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--repo-root requires a value".to_string());
                }
                repo_root = Some(v.clone());
                i += 2;
            }
            _ if !a.starts_with("--") && !kind_set => {
                kind = parse_gate_kind(a).ok_or_else(|| format!("unknown gate kind: {a}"))?;
                kind_set = true;
                i += 1;
            }
            _ => return Err(format!("unknown argument: {a}")),
        }
    }

    let write_witness = write_witness
        || std::env::var("AI_DX_WRITE_WITNESS")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    Ok((kind, dry_run, write_witness, default_repo_root(repo_root)))
}

fn parse_init_cli(args: &[String]) -> Result<(InitRequest, String), String> {
    let mut apply = false;
    let mut packs: Vec<String> = vec![];
    let mut repo_root: Option<String> = None;

    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--apply" => {
                apply = true;
                i += 1;
            }
            "--packs" => {
                let v = args.get(i + 1).ok_or_else(|| {
                    "--packs requires a value (e.g. builtin:rust,builtin:node)".to_string()
                })?;
                if v.starts_with("--") {
                    return Err(
                        "--packs requires a value (e.g. builtin:rust,builtin:node)".to_string()
                    );
                }
                for p in v.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    packs.push(p.to_string());
                }
                i += 2;
            }
            "--repo-root" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--repo-root requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--repo-root requires a value".to_string());
                }
                repo_root = Some(v.clone());
                i += 2;
            }
            _ => return Err(format!("unknown argument: {a}")),
        }
    }

    let repo_root = default_repo_root(repo_root);
    Ok((
        InitRequest {
            repo_root: Some(repo_root.clone()),
            apply: Some(apply),
            packs: if packs.is_empty() { None } else { Some(packs) },
            external_packs: None,
        },
        repo_root,
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str);

    match command {
        Some("version") | Some("--version") | Some("-V") => {
            print_version();
            return Ok(());
        }
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            return Ok(());
        }
        Some(flag) if is_v1_flag(flag) => {
            eprintln!(
                "compas: v1-style CLI flag `{flag}` removed in v2; use subcommands: init|validate|gate"
            );
            std::process::exit(2);
        }
        Some("init") => {
            let (req, repo_root) = match parse_init_cli(&args[2..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("compas: {e}");
                    std::process::exit(2);
                }
            };
            let out = ai_dx_mcp::app::compas_init(&repo_root, req);
            let out = finalize_init(out);
            println!("{}", serde_json::to_string_pretty(&out)?);
            if !out.ok {
                std::process::exit(1);
            }
            return Ok(());
        }
        Some("validate") => {
            let (mode, write_baseline, repo_root, baseline_maintenance) =
                match parse_validate_cli(&args[2..]) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("compas: {e}");
                        std::process::exit(2);
                    }
                };
            let out = ai_dx_mcp::app::validate(
                &repo_root,
                mode,
                write_baseline,
                baseline_maintenance.as_ref(),
            );
            let out = finalize_validate(out, ResponseMode::Compact);
            println!("{}", serde_json::to_string_pretty(&out)?);
            if !out.ok {
                std::process::exit(1);
            }
            return Ok(());
        }
        Some("gate") => {
            let (kind, dry_run, write_witness, repo_root) = match parse_gate_cli(&args[2..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("compas: {e}");
                    std::process::exit(2);
                }
            };
            let out = ai_dx_mcp::app::gate(&repo_root, kind, dry_run, write_witness).await;
            let out = finalize_gate(out, ResponseMode::Compact);
            println!("{}", serde_json::to_string_pretty(&out)?);
            if !out.ok {
                std::process::exit(1);
            }
            return Ok(());
        }
        Some(other)
            if matches!(other, "--stdio" | "stdio" | "--mcp" | "mcp")
                || (other == "--transport" && args.get(2).is_some_and(|v| v == "stdio"))
                || other.starts_with("--") => {}
        Some(other) => {
            eprintln!(
                "compas: unknown command `{other}`; use init|validate|gate, or no args to start MCP server"
            );
            std::process::exit(2);
        }
        None => {}
    }

    let service = AiDxServer::new()
        .serve(mcp_stdio::HybridStdioTransport::new())
        .await?;
    service.waiting().await?;
    Ok(())
}
