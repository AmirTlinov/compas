# Compas: tests + CI integration for plugin manager (interop with registry)

## Outcome
Guarantee long-term correctness:
- plugin manager behavior is covered by tests
- compas CI validates against a fixture manifest + signature
- prevents silent security regressions

## Scope IN
- Add fixture manifest/signature/public key for tests (dev key).
- Add integration tests for:
  - manifest parse
  - signature verify
  - safe extraction
  - lockfile lifecycle
- Optional: CI job that downloads latest registry release manifest and runs `plugins list` (behind feature flag / scheduled).

## Scope OUT
- Network-dependent CI as required gating (keep PR CI offline).

## Acceptance criteria
- `cargo test` covers plugin manager core.
- PR CI fails if signature verification breaks.

## Child issues
Use the stable IDs below (issue numbers vary by repo/time):
- [ ] C05C1 — Test fixtures: dev keypair + signed manifest
- [ ] C05C2 — Integration tests: lifecycle
- [ ] C05C3 — Optional scheduled smoke against latest registry release
