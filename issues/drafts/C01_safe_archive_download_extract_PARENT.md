# Compas: safe archive download + extraction (no system tar, no traversal)

## Outcome
Replace unsafe registry handling (system `tar`, permissive extraction) with a safe, bounded, deterministic extractor:
- blocks path traversal, absolute paths, symlinks, devices
- enforces size/file-count limits
- produces a canonical on-disk registry tree used for installs

## Scope IN
- Download registry archive specified by manifest (or legacy source).
- Verify sha256 matches manifest before extraction.
- Implement Rust-only extraction using `flate2` + `tar` crate.
- Enforce security rules (fail-closed).

## Scope OUT
- Plugin install into target repo (C02).
- Tiers/trust policy (C03).

## Contracts touched
- `crates/ai-dx-mcp/src/cli_plugins.rs` (registry caching/extraction)

## Deliverables
- `extract_registry_archive_safe(...)` implementation.
- Limits configuration constants (documented).
- Tests for traversal/symlink/oversize.

## Dependencies
- Manifest v1 fetch/verify (C00) should provide archive url + sha256.

## Risks
- Tar edge cases (pax headers, long paths, hardlinks) → must be explicitly blocked/handled.
- Decompression bombs → enforce total uncompressed size limit.

## Acceptance criteria
- Malicious tar with `../` or absolute paths is rejected.
- Symlink/hardlink entries are rejected.
- Extraction creates only regular files + dirs under cache dir.

## Required tests
- Unit tests: tar fixtures generated in tests.

## Non-goals
- Supporting zip/7z; tar.gz only.

## Child issues
Use the stable IDs below (issue numbers vary by repo/time):
- [ ] C01C1 — Implement safe tar.gz extractor (Rust-only)
- [ ] C01C2 — Download archive + sha256 verify before extraction
- [ ] C01C3 — Malicious archive regression tests
