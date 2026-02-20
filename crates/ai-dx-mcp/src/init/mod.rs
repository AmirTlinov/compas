//! Init pipeline (planned): the only place where external I/O like network pack download is allowed.
//!
//! Guardrail: network capability is represented as a token that is constructible only from here.

use std::path::Path;

mod apply;
mod planner;

/// Capability token that authorizes network I/O.
///
/// Guardrail: token is constructible only inside `crate::init` (tuple field is private).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct NetAllowed(());

/// Acquire the network capability token.
///
/// Intended usage: only `init` is allowed to download external packs; validate/gate must stay
/// network-free.
#[allow(dead_code)]
fn allow_network_for_init() -> NetAllowed {
    NetAllowed(())
}

#[allow(unused_imports)] // Used by compas.init (and later by apply+vendoring slices).
pub(crate) use planner::plan_init;

pub(crate) fn init(repo_root: &str, req: crate::api::InitRequest) -> crate::api::InitOutput {
    let plan = match planner::plan_init(Path::new(repo_root), &req) {
        Ok(p) => p,
        Err(e) => {
            return crate::api::InitOutput {
                ok: false,
                error: Some(e),
                repo_root: repo_root.to_string(),
                applied: false,
                plan: None,
                summary_md: None,
                payload_meta: None,
            };
        }
    };

    let apply = req.apply.unwrap_or(false);
    if apply && let Err(e) = apply::apply_plan(Path::new(repo_root), &plan) {
        return crate::api::InitOutput {
            ok: false,
            error: Some(e),
            repo_root: repo_root.to_string(),
            applied: false,
            plan: Some(plan),
            summary_md: None,
            payload_meta: None,
        };
    }

    let plan_for_output = if apply {
        // Output budget-safety: on apply success, avoid echoing full file contents back to the caller.
        // Dry-run (apply=false) keeps full contents for preview.
        crate::api::InitPlan {
            writes: plan
                .writes
                .into_iter()
                .map(|w| crate::api::InitWriteFile {
                    path: w.path,
                    content_utf8:
                        "[omitted by compas.init apply; run compas.init/apply=false to preview]"
                            .to_string(),
                })
                .collect(),
            deletes: plan.deletes,
        }
    } else {
        plan
    };

    crate::api::InitOutput {
        ok: true,
        error: None,
        repo_root: repo_root.to_string(),
        applied: apply,
        plan: Some(plan_for_output),
        summary_md: None,
        payload_meta: None,
    }
}

/// Download a pack archive from an http(s) URL.
///
/// Guardrail: requires [`NetAllowed`], so it can be called only from `init`.
///
/// Notes:
/// - Fail-closed on non-2xx, missing scheme, or oversized downloads.
/// - Uses bounded chunked read to avoid unbounded memory growth.
#[allow(dead_code)]
#[cfg(feature = "external_packs")]
pub(crate) async fn download_pack_archive_http(
    _net: NetAllowed,
    url: &str,
) -> Result<Vec<u8>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "only http(s) pack sources are supported here: {url:?}"
        ));
    }

    const MAX_BYTES: u64 = 20 * 1024 * 1024; // 20 MiB

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to GET {url:?}: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GET {url:?} failed: http status {status}"));
    }

    if let Some(len) = resp.content_length()
        && len > MAX_BYTES
    {
        return Err(format!(
            "pack archive too large for {url:?}: content_length={len} > max={MAX_BYTES}"
        ));
    }

    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("failed to read response body for {url:?}: {e}"))?
    {
        if (out.len() as u64) + (chunk.len() as u64) > MAX_BYTES {
            return Err(format!(
                "pack archive too large for {url:?}: exceeded max={MAX_BYTES} bytes"
            ));
        }
        out.extend_from_slice(&chunk);
    }

    Ok(out)
}

#[allow(dead_code)]
#[cfg(not(feature = "external_packs"))]
pub(crate) async fn download_pack_archive_http(
    _net: NetAllowed,
    url: &str,
) -> Result<Vec<u8>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!(
            "only http(s) pack sources are supported here: {url:?}"
        ));
    }
    Err("external_packs feature is disabled (compas-lite); rebuild with default-features or --features external_packs".to_string())
}

#[cfg(test)]
mod tests {
    use super::allow_network_for_init;
    use super::download_pack_archive_http;

    #[test]
    fn init_can_acquire_network_capability_token() {
        let _token = allow_network_for_init();
    }

    #[tokio::test]
    async fn download_pack_archive_http_rejects_non_http_sources() {
        let net = allow_network_for_init();
        let err = download_pack_archive_http(net, "file:///tmp/pack.tar.gz")
            .await
            .unwrap_err();
        assert!(err.contains("http"), "{err}");
    }
}
