use ai_dx_mcp::{
    api::ResponseMode,
    response::{finalize_gate, finalize_init, finalize_validate},
    server::AiDxServer,
};
use rmcp::ServiceExt;
mod cli;
mod mcp_stdio;

fn print_version() {
    println!("{}", env!("CARGO_PKG_VERSION"));
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
            cli::print_help();
            return Ok(());
        }
        Some("plugins") => {
            if cli::plugins_help_requested(&args) {
                cli::print_plugins_help();
                return Ok(());
            }
            let parsed = match cli::parse_plugins_cli(&args[2..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("compas: {e}");
                    std::process::exit(2);
                }
            };
            let code = match cli::run_plugins_cli(parsed).await {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("compas: {e}");
                    1
                }
            };
            if code != 0 {
                std::process::exit(code);
            }
            return Ok(());
        }
        Some(flag) if cli::is_v1_flag(flag) => {
            eprintln!(
                "compas: v1-style CLI flag `{flag}` removed in v2; use subcommands: init|validate|gate"
            );
            std::process::exit(2);
        }
        Some("init") => {
            let (req, repo_root) = match cli::parse_init_cli(&args[2..]) {
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
                match cli::parse_validate_cli(&args[2..]) {
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
            let (kind, dry_run, write_witness, repo_root) = match cli::parse_gate_cli(&args[2..]) {
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
                "compas: unknown command `{other}`; use init|validate|gate|plugins, or no args to start MCP server"
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
