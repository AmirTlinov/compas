# Compas: native plugin manager v1 — manifest fetch + signature verification (fail-closed)

## Outcome
Make `compas plugins ...` secure-by-default and community scalable:
- compas consumes a **versioned registry manifest** (not arbitrary registry code)
- compas **verifies registry authenticity offline** (signature + sha256)
- compas can list/info/install/update/uninstall without Python installer scripts

## Scope IN
- Add manifest model + parser (Manifest v1).
- Add trust root:
  - embedded public key(s) for the official registry
  - offline verification of `registry.manifest.v1.json.sig`
- Support registry sources:
  - default: `releases/latest/download/registry.manifest.v1.json`
  - local file path (manifest json)
  - optional: local directory (registry checkout) for dev
- Fail-closed errors with actionable messages.

## Scope OUT
- Safe tar extraction / install atomicity (C01/C02).
- Policy over tiers/packs (C03).
- Website/catalog (registry repo).

## Contracts touched
- `crates/ai-dx-mcp/src/cli.rs` (default registry source changes)
- New module(s): `crates/ai-dx-mcp/src/plugins/manifest.rs`, `.../trust.rs` (names TBD)
- Docs: `ARCHITECTURE.md` / `AGENTS.md` plugin section

## Deliverables
- `compas plugins list/info` implemented without Python.
- `compas plugins install` reads manifest and resolves archive URL + sha.
- Signature verification matches registry release signing exactly.

## Dependencies
- Registry repo must ship signed manifest assets (R00).

## Risks
- Crypto mismatches (cosign sign-blob semantics) → must have golden interop tests.
- Backward compatibility: existing `--registry <tar.gz>` usage might break → provide migration path.

## Acceptance criteria
- Given `registry.manifest.v1.json` + `.sig` + embedded pubkey:
  - compas verifies signature and rejects tampered bytes.
- `compas plugins list` shows plugins from manifest.
- No Python execution required for list/info/install path.

## Required tests
- Unit tests for signature verify (positive + negative).
- Integration test that downloads a fixture manifest and validates (offline, local file fixtures).

## Non-goals
- Keyless verification / OIDC.
- Per-plugin artifact distribution.

## Child issues
Use the stable IDs below (issue numbers vary by repo/time):
- [ ] C00C1 — Manifest v1 model + parser + invariants
- [ ] C00C2 — Offline signature verification (cosign sign-blob interop)
- [ ] C00C3 — Manifest fetch + cache (bounded, deterministic)
- [ ] C00C4 — CLI migration + help updates (manifest-based registry)
