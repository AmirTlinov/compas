# STATUS (SSOT) — compas core

This page is the **single source of truth** for “what is production‑ready today” in the `compas` core repository.

If something is not written here (or in the referenced docs), treat it as **non‑contractual / subject to change**.

## What compas is (core scope)

Compas is a **fail‑closed quality engine** for AI agents:

- `validate`: deterministic repo checks (ratchet/strict/warn)
- `gate`: `validate(ratchet)` + execution of a deterministic tool chain + receipts/witness
- `init`: bootstrap repo-side config (`.agents/mcp/compas/*`)
- `catalog`: show effective plugin/tool wiring + ownership
- `exec`: run tool_id under strict policy with bounded I/O

Core design rule:
- **compas core does not contain community plugins**. It only provides runtime + validation + installation plumbing.

## Community plugins (external SSOT)

Community plugins are maintained in a separate repository:
- `AmirTlinov/compas-plugin-registry`

That repository is the SSOT for:
- plugin package layout and plugin spec
- registry manifest v1 schema and signing flow
- curated packs and governance metadata
- registry CI and e2e harness

## Native plugin manager (production‑ready)

Compas supports native plugin installs from a **signed registry manifest v1**:

- default registry source:
  - `https://github.com/AmirTlinov/compas-plugin-registry/releases/latest/download/registry.manifest.v1.json`
- verification:
  - verifies `registry.manifest.v1.json.sig` offline (cosign sign‑blob ECDSA P‑256 semantics)
  - verifies archive sha256 from the manifest
- extraction:
  - Rust‑native tar.gz extractor (no system `tar`)
  - rejects path traversal (`..`), symlinks, unsupported entry types, multiple top-level roots
  - bounded by MAX_ENTRIES / MAX_TOTAL_BYTES / per-file limits
- state:
  - lockfile: `.agents/mcp/compas/plugins.lock.json`
  - drift detection + fail‑closed installs/updates/uninstalls unless `--force`
- governance policy:
  - blocks `tier=experimental` unless `--allow-experimental`
  - blocks `tier=deprecated` (or deprecated metadata) unless `--allow-deprecated`

Usage docs:
- `docs/PLUGINS.md`

## CI and regression protection (production‑ready)

This repo has GitHub Actions CI:
- `cargo fmt --check`
- `cargo test`

Security/regression test coverage for plugin manager includes:
- signature verification (valid + tampered manifest)
- archive sha256 mismatch
- malicious tar entries (symlink / traversal)
- tier policy enforcement

## Known non-goals / out-of-scope

- Per‑plugin artifact distribution (registry ships a single archive v1).
- Online transparency logs / TUF delegation (can be layered later).
- Enforcing “@codex review comment” mechanically (process convention, not a technical invariant).

## Operational notes (AI-only workflows)

If you run AI‑only development:

- Keep `ci_fast` deterministic and cheap.
- Use `gate` for “proof of work” (receipts + witness).
- Treat plugin installs as **policy changes**; pin registry versions for CI/reproducibility.

