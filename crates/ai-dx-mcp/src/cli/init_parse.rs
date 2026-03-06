use ai_dx_mcp::api::InitRequest;

use super::default_repo_root;

pub(crate) fn parse_init_cli(args: &[String]) -> Result<(InitRequest, String), String> {
    let mut apply = false;
    let mut profile: Option<String> = None;
    let mut registry_source: Option<String> = None;
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
            "--profile" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| "--profile requires a value (e.g. ai_first)".to_string())?;
                if v.starts_with("--") {
                    return Err("--profile requires a value (e.g. ai_first)".to_string());
                }
                profile = Some(v.clone());
                i += 2;
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
            profile,
            registry_source,
            packs: if packs.is_empty() { None } else { Some(packs) },
            external_packs: None,
        },
        repo_root,
    ))
}
