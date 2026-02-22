use base64::Engine as _;
use base64::engine::general_purpose;
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;
use p256::pkcs8::EncodePublicKey;
use p256::pkcs8::LineEnding;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir parent");
    }
    std::fs::write(path, content).expect("write file");
}

fn run_compas(args: &[String]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ai-dx-mcp");
    let cache = tempfile::tempdir().expect("temp cache");
    std::process::Command::new(bin)
        .env("XDG_CACHE_HOME", cache.path())
        .args(args)
        .output()
        .expect("run compas")
}

fn fixture_manifest_bytes() -> String {
    // This fixture is signed exactly as written; do not reformat.
    // NOTE: install/update would also require a matching archive file, but list/info only validate
    // the manifest schema, so we keep it minimal here.
    [
        "{",
        "  \"schema\": \"compas.registry.manifest.v1\",",
        "  \"registry_version\": \"fixture-1\",",
        "  \"archive\": {",
        "    \"name\": \"compas_plugins-fixture.tar.gz\",",
        "    \"sha256\": \"0000000000000000000000000000000000000000000000000000000000000000\"",
        "  },",
        "  \"plugins\": [",
        "    {",
        "      \"id\": \"spec-adr-gate\",",
        "      \"aliases\": [\"spec-gate\"],",
        "      \"path\": \"plugins/spec-adr-gate\",",
        "      \"description\": \"Fixture plugin for signature verification tests\",",
        "      \"package\": {",
        "        \"version\": \"0.1.0\",",
        "        \"type\": \"script\",",
        "        \"maturity\": \"stable\",",
        "        \"runtime\": \"python3\",",
        "        \"portable\": true,",
        "        \"languages\": [\"agnostic\"],",
        "        \"entrypoint\": \"README.md\",",
        "        \"license\": \"MIT\"",
        "      },",
        "      \"tier\": \"community\",",
        "      \"maintainers\": [\"AmirTlinov\"],",
        "      \"tags\": [\"quality\"],",
        "      \"compat\": {\"compas\": {\"min\": \"0.1.0\", \"max\": null}}",
        "    }",
        "  ],",
        "  \"packs\": [",
        "    {",
        "      \"id\": \"core\",",
        "      \"description\": \"Fixture pack\",",
        "      \"plugins\": [\"spec-adr-gate\"]",
        "    }",
        "  ]",
        "}",
        "",
    ]
    .join("\n")
}

fn sign_manifest_b64(manifest_bytes: &[u8]) -> (String, String) {
    // Deterministic test-only key material:
    // - Scalar = 1 (valid, stable, and does not rely on RNG in tests).
    let mut scalar_bytes = [0u8; 32];
    scalar_bytes[31] = 1;
    let signing_key = SigningKey::from_bytes(&scalar_bytes.into()).expect("signing key");
    let sig: p256::ecdsa::Signature = signing_key.sign(manifest_bytes);
    let sig_der = sig.to_der();
    let sig_b64 = general_purpose::STANDARD.encode(sig_der.as_bytes());

    let pubkey_pem = signing_key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .expect("pubkey pem");

    (sig_b64, pubkey_pem)
}

fn write_manifest_fixture(dir: &Path, manifest: &str, sig_b64: &str, pubkey_pem: &str) -> PathBuf {
    let manifest_path = dir.join("registry.manifest.v1.json");
    let sig_path = dir.join("registry.manifest.v1.json.sig");
    let pubkey_path = dir.join("pubkey.pem");
    write_file(&manifest_path, manifest);
    write_file(&sig_path, &format!("{sig_b64}\n"));
    write_file(&pubkey_path, pubkey_pem);
    pubkey_path
}

#[test]
fn plugins_list_verifies_signature_with_pubkey_override() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path();

    let manifest = fixture_manifest_bytes();
    let (sig_b64, pubkey_pem) = sign_manifest_b64(manifest.as_bytes());
    let pubkey_path = write_manifest_fixture(dir, &manifest, &sig_b64, &pubkey_pem);

    let args = vec![
        "plugins".to_string(),
        "list".to_string(),
        "--registry".to_string(),
        dir.join("registry.manifest.v1.json")
            .to_string_lossy()
            .to_string(),
        "--".to_string(),
        "--json".to_string(),
        "--pubkey".to_string(),
        pubkey_path.to_string_lossy().to_string(),
    ];
    let out = run_compas(&args);
    assert!(
        out.status.success(),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let payload: Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let arr = payload.as_array().expect("list payload array");
    assert!(
        arr.iter()
            .any(|v| v.get("id").and_then(|x| x.as_str()) == Some("spec-adr-gate")),
        "plugins list missing spec-adr-gate: {arr:?}"
    );
}

#[test]
fn plugins_list_rejects_tampered_manifest() {
    let workspace = tempfile::tempdir().expect("workspace");
    let dir = workspace.path();

    let mut manifest = fixture_manifest_bytes();
    let (sig_b64, pubkey_pem) = sign_manifest_b64(manifest.as_bytes());
    let pubkey_path = write_manifest_fixture(dir, &manifest, &sig_b64, &pubkey_pem);

    // Tamper with manifest bytes after signing.
    manifest = manifest.replace("fixture-1", "fixture-2");
    write_file(&dir.join("registry.manifest.v1.json"), &manifest);

    let args = vec![
        "plugins".to_string(),
        "list".to_string(),
        "--registry".to_string(),
        dir.join("registry.manifest.v1.json")
            .to_string_lossy()
            .to_string(),
        "--".to_string(),
        "--json".to_string(),
        "--pubkey".to_string(),
        pubkey_path.to_string_lossy().to_string(),
    ];
    let out = run_compas(&args);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid manifest signature") || stderr.contains("signature"),
        "unexpected stderr: {stderr}"
    );
}
