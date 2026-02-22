# [C00] Offline signature verification (cosign sign-blob interop)

## Concrete scope
Implement offline verification in Rust that matches registry signing:
- Manifest is signed by `cosign sign-blob` (key-based).
- Signature file is base64-encoded DER ECDSA signature.
- Verification must be done without calling `cosign` binary.

**Signing contract must be frozen** (interop-critical):
- Registry signs **exact raw bytes** of `registry.manifest.v1.json` as stored in release asset (no JSON reformatting).
- Signing command (registry CI):
  - `cosign sign-blob --key cosign.key --output-signature registry.manifest.v1.json.sig registry.manifest.v1.json`
- Verification semantics to match cosign:
  - ECDSA P-256 **with SHA-256** over the blob bytes.

## Interface boundary
- Input: `manifest_bytes`, `signature_b64`, embedded public key PEM.
- Output: `Ok(())` or typed error (invalid signature / unsupported key / parse error).

## Implementation steps
1) Add dependency:
   - `p256` + `ecdsa` (or `ring` if it can parse ECDSA P-256 DER sig easily).
2) Parse public key:
   - PEM to `p256::PublicKey` (SPKI).
3) Parse signature:
   - base64 decode
   - parse DER ECDSA signature to `(r,s)` form.
4) Verify over `SHA256(manifest_bytes)` using ECDSA P-256.
5) Add keyring support (future rotation):
   - allow multiple embedded public keys; verify succeeds if any matches.
   - compute and return `key_id` (e.g. SHA256 over SPKI bytes) for lockfile auditing.

## Test checklist
- Golden interop fixture (required):
  - generate a dev keypair with `cosign generate-key-pair`
  - generate `manifest.json` bytes + `.sig` using `cosign sign-blob`
  - commit these as test fixtures and ensure Rust verification succeeds
- Secondary positive fixture:
  - sign bytes using Rust `p256` signing and verify (ensures our own plumbing works)
- Negative fixtures:
  - flip one byte in manifest
  - wrong signature
  - wrong key

## Definition of Done
- Signature verification is deterministic, offline, and matches registry workflow.
- We have at least one **cosign-generated** test vector proving interop.

Parent: #28
