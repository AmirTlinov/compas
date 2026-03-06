use base64::{Engine as _, engine::general_purpose};
use p256::ecdsa::{Signature as P256Signature, VerifyingKey, signature::Verifier};
use p256::pkcs8::DecodePublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

const TIER_EXPERIMENTAL: &str = "experimental";
const TIER_SUNSET: &str = "sunset";
const SUNSET_META_COMPAT_KEY: &str = concat!("deprecat", "ed");
const PACK_RUNTIME_KIND_MIXED: &str = "mixed";

pub const OFFICIAL_REGISTRY_COSIGN_PUBKEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAExWXyUnb9j+0nAopQJWPU2JObKitu\nfNacvZOK6C4P/AeUOQc0PmK3rSrm/NRII6pCRssOC65QTbt+0zi0dzySwQ==\n-----END PUBLIC KEY-----\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryManifestV1 {
    pub schema: String,
    pub registry_version: String,
    pub archive: RegistryArchiveV1,
    pub plugins: Vec<RegistryPluginV1>,
    pub packs: Vec<RegistryPackV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryArchiveV1 {
    pub name: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPluginV1 {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub path: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub description: String,
    pub package: RegistryPluginPackageV1,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub maintainers: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub compat: Option<serde_json::Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPluginPackageV1 {
    pub version: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub maturity: String,
    pub runtime: String,
    pub portable: bool,
    #[serde(default)]
    pub languages: Vec<String>,
    pub entrypoint: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackV1 {
    pub id: String,
    pub description: String,
    pub plugins: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub runtime_kind: String,
    #[serde(default)]
    pub cost_class: String,
    #[serde(default)]
    pub recommendation: Option<RegistryPackRecommendationV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackRecommendationV1 {
    pub priority: u32,
    pub why: String,
    #[serde(default)]
    pub languages_any: Vec<String>,
    #[serde(default)]
    pub languages_all: Vec<String>,
    #[serde(default)]
    pub signals_any: Vec<String>,
    #[serde(default)]
    pub signals_all: Vec<String>,
    #[serde(default)]
    pub when_no_languages: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ManifestResolved {
    pub manifest: RegistryManifestV1,
    pub manifest_sha256: String,
    pub signature_key_id: Option<String>,
    pub base_url: Option<String>,
    pub base_dir: Option<PathBuf>,
}

pub fn is_http_url(raw: &str) -> bool {
    raw.starts_with("https://") || raw.starts_with("http://")
}

fn sha256_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    format!("{:x}", hasher.finalize())
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_compas_id(s: &str, min_len: usize) -> bool {
    let s = s.trim();
    if s.len() < min_len || s.len() > 64 {
        return false;
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'a'..='z' | '0'..='9') {
        return false;
    }
    for c in chars {
        if !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') {
            return false;
        }
    }
    true
}

fn validate_token_list(
    values: &[String],
    context: &str,
    min_token_len: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if values.is_empty() && !allow_empty {
        return Err(format!("{context} must be non-empty"));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for value in values {
        if !is_compas_id(value, min_token_len) {
            return Err(format!("{context} has invalid token: {value:?}"));
        }
        if !seen.insert(value.clone()) {
            return Err(format!("{context} contains duplicate token: {value:?}"));
        }
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    if sorted != values {
        return Err(format!("{context} must be sorted lexicographically"));
    }
    Ok(())
}

fn validate_pack_recommendation(
    pack_id: &str,
    rec: &RegistryPackRecommendationV1,
) -> Result<(), String> {
    if rec.priority == 0 {
        return Err(format!(
            "pack {pack_id} recommendation.priority must be positive integer"
        ));
    }
    let why_len = rec.why.trim().chars().count();
    if !(12..=220).contains(&why_len) {
        return Err(format!(
            "pack {pack_id} recommendation.why length must be 12..220 chars"
        ));
    }
    validate_token_list(
        &rec.languages_any,
        &format!("pack {pack_id} recommendation.languages_any"),
        1,
        true,
    )?;
    validate_token_list(
        &rec.languages_all,
        &format!("pack {pack_id} recommendation.languages_all"),
        1,
        true,
    )?;
    validate_token_list(
        &rec.signals_any,
        &format!("pack {pack_id} recommendation.signals_any"),
        2,
        true,
    )?;
    validate_token_list(
        &rec.signals_all,
        &format!("pack {pack_id} recommendation.signals_all"),
        2,
        true,
    )?;
    if rec.languages_any.is_empty()
        && rec.languages_all.is_empty()
        && rec.signals_any.is_empty()
        && rec.signals_all.is_empty()
        && !rec.when_no_languages
    {
        return Err(format!(
            "pack {pack_id} recommendation must define at least one selector"
        ));
    }
    if rec.when_no_languages && (!rec.languages_any.is_empty() || !rec.languages_all.is_empty()) {
        return Err(format!(
            "pack {pack_id} recommendation.when_no_languages cannot be combined with language selectors"
        ));
    }
    Ok(())
}

fn has_pack_aggregate_metadata(pack: &RegistryPackV1) -> bool {
    !pack.capabilities.is_empty()
        || !pack.requires.is_empty()
        || !pack.runtime_kind.trim().is_empty()
        || !pack.cost_class.trim().is_empty()
}

pub fn validate_manifest_v1(manifest: &RegistryManifestV1) -> Result<(), String> {
    if manifest.schema != "compas.registry.manifest.v1" {
        return Err(format!(
            "unsupported registry manifest schema: {}",
            manifest.schema
        ));
    }
    if manifest.registry_version.trim().is_empty() {
        return Err("registry manifest has empty registry_version".to_string());
    }
    if manifest.archive.name.trim().is_empty()
        || manifest.archive.name.contains('/')
        || manifest.archive.name.contains('\\')
    {
        return Err(format!(
            "invalid manifest archive.name (must be a file name): {}",
            manifest.archive.name
        ));
    }
    if !is_sha256_hex(&manifest.archive.sha256) {
        return Err(format!(
            "invalid manifest archive.sha256 (expected 64 lowercase hex chars): {}",
            manifest.archive.sha256
        ));
    }
    if manifest.plugins.is_empty() {
        return Err("registry manifest has empty plugins list".to_string());
    }
    if manifest.packs.is_empty() {
        return Err("registry manifest has empty packs list".to_string());
    }

    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut aliases: BTreeSet<String> = BTreeSet::new();
    for plugin in &manifest.plugins {
        if !is_compas_id(&plugin.id, 2) {
            return Err(format!("invalid plugin id in manifest: {}", plugin.id));
        }
        if !ids.insert(plugin.id.clone()) {
            return Err(format!("duplicate plugin id in manifest: {}", plugin.id));
        }
        for alias in &plugin.aliases {
            if !is_compas_id(alias, 2) {
                return Err(format!("plugin {} has invalid alias: {}", plugin.id, alias));
            }
            if ids.contains(alias) {
                return Err(format!(
                    "plugin {} alias collides with canonical plugin id: {}",
                    plugin.id, alias
                ));
            }
            if !aliases.insert(alias.clone()) {
                return Err(format!("duplicate alias in manifest: {}", alias));
            }
        }
        let plugin_path = Path::new(&plugin.path);
        if plugin_path.as_os_str().is_empty() || plugin_path.is_absolute() {
            return Err(format!(
                "plugin {} has unsafe path: {}",
                plugin.id, plugin.path
            ));
        }
        if plugin.path.contains('\\') {
            return Err(format!(
                "plugin {} has unsafe path (backslashes forbidden): {}",
                plugin.id, plugin.path
            ));
        }
        for c in plugin_path.components() {
            match c {
                Component::CurDir | Component::Normal(_) => {}
                _ => {
                    return Err(format!(
                        "plugin {} has unsafe path: {}",
                        plugin.id, plugin.path
                    ));
                }
            }
        }
        if plugin.package.version.trim().is_empty() {
            return Err(format!("plugin {} has empty package.version", plugin.id));
        }
        if plugin.package.entrypoint.trim().is_empty() {
            return Err(format!("plugin {} has empty package.entrypoint", plugin.id));
        }
        let languages = &plugin.package.languages;
        validate_token_list(
            languages,
            &format!("plugin {} package.languages", plugin.id),
            1,
            false,
        )?;

        let capabilities = plugin
            .extra
            .get("capabilities")
            .and_then(|value| value.as_array())
            .ok_or_else(|| format!("plugin {} capabilities missing or invalid", plugin.id))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToString::to_string)
                    .ok_or_else(|| format!("plugin {} capabilities contains non-string", plugin.id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_token_list(
            &capabilities,
            &format!("plugin {} capabilities", plugin.id),
            2,
            false,
        )?;

        let requires = plugin
            .extra
            .get("requires")
            .and_then(|value| value.as_array())
            .ok_or_else(|| format!("plugin {} requires missing or invalid", plugin.id))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToString::to_string)
                    .ok_or_else(|| format!("plugin {} requires contains non-string", plugin.id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_token_list(
            &requires,
            &format!("plugin {} requires", plugin.id),
            2,
            true,
        )?;

        let runtime_kind = plugin
            .extra
            .get("runtime_kind")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("plugin {} runtime_kind missing or invalid", plugin.id))?;
        if !matches!(runtime_kind, "tool-backed" | "hybrid" | "reference") {
            return Err(format!(
                "plugin {} runtime_kind invalid: {}",
                plugin.id, runtime_kind
            ));
        }
        if runtime_kind != plugin.package.kind {
            return Err(format!(
                "plugin {} runtime_kind must match package.type",
                plugin.id
            ));
        }
        let cost_class = plugin
            .extra
            .get("cost_class")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("plugin {} cost_class missing or invalid", plugin.id))?;
        if !matches!(cost_class, "low" | "medium" | "high") {
            return Err(format!(
                "plugin {} cost_class invalid: {}",
                plugin.id, cost_class
            ));
        }

        let sunset_meta_present = plugin
            .extra
            .get(SUNSET_META_COMPAT_KEY)
            .as_ref()
            .and_then(|value| value.as_object())
            .is_some_and(|obj| !obj.is_empty());
        let tier = plugin
            .tier
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match tier.as_str() {
            "" | "community" | "certified" | TIER_EXPERIMENTAL | TIER_SUNSET => {}
            other => return Err(format!("plugin {} has invalid tier {}", plugin.id, other)),
        }
        if sunset_meta_present && tier != TIER_SUNSET {
            return Err(format!(
                "plugin {} sunset metadata requires tier=sunset",
                plugin.id
            ));
        }
    }

    let plugin_id_set: BTreeSet<String> = manifest.plugins.iter().map(|p| p.id.clone()).collect();
    for pack in &manifest.packs {
        if !is_compas_id(&pack.id, 2) {
            return Err(format!("invalid pack id in manifest: {}", pack.id));
        }
        if pack.description.trim().len() < 8 {
            return Err(format!("pack {} description too short", pack.id));
        }
        if pack.plugins.is_empty() {
            return Err(format!("pack {} has empty plugins list", pack.id));
        }
        let mut local_refs = BTreeSet::new();
        for plugin_id in &pack.plugins {
            if !plugin_id_set.contains(plugin_id) {
                return Err(format!(
                    "pack {} references unknown plugin id: {}",
                    pack.id, plugin_id
                ));
            }
            if !local_refs.insert(plugin_id.clone()) {
                return Err(format!(
                    "pack {} has duplicate plugin reference: {}",
                    pack.id, plugin_id
                ));
            }
        }
        let mut sorted_plugins = pack.plugins.clone();
        sorted_plugins.sort();
        if sorted_plugins != pack.plugins {
            return Err(format!("pack {} plugins must be sorted by id", pack.id));
        }
        if has_pack_aggregate_metadata(pack) {
            if pack.capabilities.is_empty() {
                return Err(format!(
                    "pack {} capabilities must be non-empty when aggregate metadata is present",
                    pack.id
                ));
            }
            if pack.runtime_kind.trim().is_empty() {
                return Err(format!(
                    "pack {} runtime_kind missing while aggregate metadata is present",
                    pack.id
                ));
            }
            if pack.cost_class.trim().is_empty() {
                return Err(format!(
                    "pack {} cost_class missing while aggregate metadata is present",
                    pack.id
                ));
            }
            validate_token_list(
                &pack.capabilities,
                &format!("pack {} capabilities", pack.id),
                2,
                false,
            )?;
            validate_token_list(
                &pack.requires,
                &format!("pack {} requires", pack.id),
                2,
                true,
            )?;
            if !matches!(
                pack.runtime_kind.as_str(),
                "tool-backed" | "hybrid" | "reference" | PACK_RUNTIME_KIND_MIXED
            ) {
                return Err(format!(
                    "pack {} runtime_kind invalid: {}",
                    pack.id, pack.runtime_kind
                ));
            }
            if !matches!(pack.cost_class.as_str(), "low" | "medium" | "high") {
                return Err(format!(
                    "pack {} cost_class invalid: {}",
                    pack.id, pack.cost_class
                ));
            }
        }
        if let Some(recommendation) = &pack.recommendation {
            validate_pack_recommendation(&pack.id, recommendation)?;
        }
    }

    let mut sorted_pack_ids: Vec<String> =
        manifest.packs.iter().map(|pack| pack.id.clone()).collect();
    let current_pack_ids = sorted_pack_ids.clone();
    sorted_pack_ids.sort();
    if current_pack_ids != sorted_pack_ids {
        return Err("manifest.packs must be sorted by id (determinism requirement)".to_string());
    }

    Ok(())
}

fn verify_cosign_blob_signature(
    payload: &[u8],
    signature_b64: &str,
    pubkey_pem: &str,
) -> Result<String, String> {
    let signature_raw = general_purpose::STANDARD
        .decode(signature_b64.trim())
        .map_err(|e| format!("failed to decode base64 signature: {e}"))?;

    let signature = P256Signature::from_der(&signature_raw)
        .map_err(|e| format!("failed to parse DER signature: {e}"))?;
    let verifying_key = VerifyingKey::from_public_key_pem(pubkey_pem)
        .map_err(|e| format!("failed to parse PEM public key: {e}"))?;

    verifying_key
        .verify(payload, &signature)
        .map_err(|e| format!("signature verification failed: {e}"))?;

    let uncompressed = verifying_key.to_encoded_point(false);
    let key_id = sha256_hex(uncompressed.as_bytes());
    Ok(format!("sha256:{key_id}"))
}

fn extract_base_url(url: &str) -> Option<String> {
    let (base, _tail) = url.rsplit_once('/')?;
    Some(base.to_string())
}

fn signature_source_for_manifest_source(source: &str) -> String {
    format!("{source}.sig")
}

#[cfg(feature = "full")]
async fn fetch_url_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to download {url}: {e}"))?;
    let response = response
        .error_for_status()
        .map_err(|e| format!("download failed for {url}: {e}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read body from {url}: {e}"))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "downloaded payload too large from {url}: {} bytes (max {max_bytes})",
            bytes.len()
        ));
    }
    Ok(bytes.to_vec())
}

#[cfg(not(feature = "full"))]
async fn fetch_url_bytes(url: &str, _max_bytes: usize) -> Result<Vec<u8>, String> {
    Err(format!(
        "URL registry sources are unavailable in lite build ({url}); use local --registry path"
    ))
}

pub async fn load_verified_manifest_source(
    registry_source: &str,
    allow_unsigned: bool,
    pubkey_pem_override: Option<String>,
) -> Result<ManifestResolved, String> {
    let registry_source = registry_source.trim().to_string();
    if registry_source.is_empty() {
        return Err("registry source must be non-empty".to_string());
    }

    let manifest_bytes: Vec<u8>;
    let mut signature_b64: Option<String> = None;
    let mut base_url: Option<String> = None;
    let mut base_dir: Option<PathBuf> = None;

    if is_http_url(&registry_source) {
        manifest_bytes = fetch_url_bytes(&registry_source, 5 * 1024 * 1024).await?;
        if !allow_unsigned {
            let sig_url = signature_source_for_manifest_source(&registry_source);
            let sig_bytes = fetch_url_bytes(&sig_url, 512 * 1024).await?;
            signature_b64 = Some(
                String::from_utf8(sig_bytes)
                    .map_err(|e| format!("signature is not valid UTF-8: {e}"))?,
            );
        }
        base_url = extract_base_url(&registry_source);
    } else {
        let path = PathBuf::from(&registry_source);
        let path = fs::canonicalize(&path)
            .map_err(|e| format!("failed to resolve registry source {}: {e}", path.display()))?;
        manifest_bytes = fs::read(&path)
            .map_err(|e| format!("failed to read manifest {}: {e}", path.display()))?;
        let sig_path = path.with_extension(format!(
            "{}.sig",
            path.extension().and_then(|s| s.to_str()).unwrap_or("json")
        ));
        if sig_path.is_file() {
            signature_b64 =
                Some(fs::read_to_string(&sig_path).map_err(|e| {
                    format!("failed to read signature {}: {e}", sig_path.display())
                })?);
        }
        base_dir = path.parent().map(PathBuf::from);
    }

    let manifest_sha256 = sha256_hex(&manifest_bytes);
    let manifest: RegistryManifestV1 = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("failed to parse registry manifest JSON: {e}"))?;
    validate_manifest_v1(&manifest)?;

    let signature_key_id = if allow_unsigned {
        None
    } else {
        let sig = signature_b64.as_deref().ok_or_else(|| {
            "missing registry manifest signature (.sig); use allow_unsigned to bypass".to_string()
        })?;
        let pubkey_pem =
            pubkey_pem_override.unwrap_or_else(|| OFFICIAL_REGISTRY_COSIGN_PUBKEY_PEM.to_string());
        Some(verify_cosign_blob_signature(
            &manifest_bytes,
            sig,
            &pubkey_pem,
        )?)
    };

    Ok(ManifestResolved {
        manifest,
        manifest_sha256,
        signature_key_id,
        base_url,
        base_dir,
    })
}
