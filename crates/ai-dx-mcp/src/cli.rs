use ai_dx_mcp::api::{BaselineMaintenance, GateKind, InitRequest, ValidateMode};

#[path = "cli_plugins.rs"]
mod plugins_impl;

const DEFAULT_PLUGIN_REGISTRY_SOURCE: &str = "https://github.com/AmirTlinov/compas-plugin-registry/releases/latest/download/registry.manifest.v1.json";
const PLUGIN_REGISTRY_ENV: &str = "COMPAS_PLUGIN_REGISTRY";

pub(crate) fn print_help() {
    println!(
        "Usage:\n  compas_mcp help\n  compas_mcp version\n  compas_mcp init [--apply] [--packs <builtin:...,...>] [--repo-root <path>]\n  compas_mcp validate [ratchet|strict|warn] [--write-baseline] [--baseline-reason <text>] [--baseline-owner <id>] [--repo-root <path>]\n  compas_mcp gate [ci-fast|ci|flagship] [--dry-run] [--write-witness] [--repo-root <path>]\n  compas_mcp plugins [install|update|uninstall|list|packs|info|doctor] [--registry <url-or-path>] [--repo-root <path>] [--allow-experimental] [--allow-deprecated] [-- <registry-installer-args...>]\n\nNotes:\n  - No args => start MCP server over stdio.\n  - v1-style flags --init/--validate/--gate are removed in v2.\n  - Defaults via env:\n      AI_DX_REPO_ROOT=<path>\n      AI_DX_WRITE_WITNESS=1|true\n      COMPAS_PLUGIN_REGISTRY=<url-or-path>\n  - Use `compas_mcp plugins ...` for native-manifest vs legacy-installer mode details.\n\nExamples:\n  compas_mcp init --apply\n  compas_mcp validate ratchet\n  compas_mcp validate ratchet --write-baseline --baseline-reason \"Quarterly baseline refresh after policy change\" --baseline-owner team-lead\n  compas_mcp gate ci-fast --dry-run\n  compas_mcp plugins list -- --json\n  compas_mcp plugins packs -- --json\n  compas_mcp plugins info spec-adr-gate\n  compas_mcp plugins install --plugins spec-adr-gate\n  compas_mcp plugins install --plugins experimental-plugin --allow-experimental\n  compas_mcp plugins update --plugins deprecated-plugin --allow-deprecated\n"
    );
}

pub(crate) fn print_plugins_help() {
    println!(
        "Usage:\n  compas_mcp plugins [install|update|uninstall|list|packs|info|doctor] [--registry <url-or-path>] [--repo-root <path>] [--allow-experimental] [--allow-deprecated] [-- <registry-installer-args...>]\n\nDefaults:\n  --registry: $COMPAS_PLUGIN_REGISTRY or {}\n  --repo-root: $AI_DX_REPO_ROOT or .\n\nNotes:\n  - Native registries are signed JSON manifests (preferred).\n  - Legacy registries are tar.gz archives or directories containing `scripts/compas_plugins.py`.\n  - install/update with native registries enforce policy:\n      - tier=experimental requires --allow-experimental\n      - tier=deprecated (or deprecated metadata) requires --allow-deprecated\n  - For native registries, these flags apply only to install/update.\n  - Legacy registries ignore --allow-experimental/--allow-deprecated (syntax-compatible passthrough).\n\nExamples:\n  compas_mcp plugins list -- --json\n  compas_mcp plugins packs -- --json\n  compas_mcp plugins info spec-adr-gate\n  compas_mcp plugins install --plugins spec-adr-gate\n  compas_mcp plugins install --plugins experimental-plugin --allow-experimental\n  compas_mcp plugins update --plugins deprecated-plugin --allow-deprecated\n",
        DEFAULT_PLUGIN_REGISTRY_SOURCE
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

pub(crate) fn is_v1_flag(arg: &str) -> bool {
    matches!(arg, "--init" | "--validate" | "--gate")
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PluginsAction {
    Install,
    Update,
    Uninstall,
    List,
    Packs,
    Info,
    Doctor,
}

impl PluginsAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "install" => Some(Self::Install),
            "update" => Some(Self::Update),
            "uninstall" => Some(Self::Uninstall),
            "list" => Some(Self::List),
            "packs" => Some(Self::Packs),
            "info" => Some(Self::Info),
            "doctor" => Some(Self::Doctor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PluginsCli {
    pub(crate) action: PluginsAction,
    pub(crate) registry_source: String,
    pub(crate) repo_root: String,
    pub(crate) installer_args: Vec<String>,
}

pub(crate) fn parse_plugins_cli(args: &[String]) -> Result<PluginsCli, String> {
    let action_raw = args.first().ok_or_else(|| {
        "plugins requires subcommand: install|update|uninstall|list|packs|info|doctor".to_string()
    })?;
    let action = PluginsAction::from_str(action_raw)
        .ok_or_else(|| format!("unknown plugins command: {action_raw}"))?;

    let mut registry_source: Option<String> = None;
    let mut repo_root: Option<String> = None;
    let mut installer_args: Vec<String> = Vec::new();

    let mut i = 1usize;
    let mut passthrough = false;
    while i < args.len() {
        let a = &args[i];
        if passthrough {
            installer_args.push(a.clone());
            i += 1;
            continue;
        }
        match a.as_str() {
            "--" => {
                passthrough = true;
                i += 1;
            }
            "--registry" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--registry requires a value".to_string())?;
                if v.starts_with("--") {
                    return Err("--registry requires a value".to_string());
                }
                registry_source = Some(v.clone());
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
            _ => {
                installer_args.push(a.clone());
                i += 1;
            }
        }
    }

    let registry_source = registry_source
        .or_else(|| std::env::var(PLUGIN_REGISTRY_ENV).ok())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PLUGIN_REGISTRY_SOURCE.to_string());

    Ok(PluginsCli {
        action,
        registry_source,
        repo_root: default_repo_root(repo_root),
        installer_args,
    })
}

pub(crate) fn plugins_help_requested(args: &[String]) -> bool {
    args.len() == 2
        || args
            .get(2)
            .is_some_and(|a| matches!(a.as_str(), "help" | "--help" | "-h"))
}

pub(crate) async fn run_plugins_cli(parsed: PluginsCli) -> Result<i32, String> {
    plugins_impl::run_plugins_cli(&parsed).await
}

pub(crate) fn parse_validate_cli(
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

pub(crate) fn parse_gate_cli(args: &[String]) -> Result<(GateKind, bool, bool, String), String> {
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

pub(crate) fn parse_init_cli(args: &[String]) -> Result<(InitRequest, String), String> {
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
