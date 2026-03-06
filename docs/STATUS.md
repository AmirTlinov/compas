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
- Core also keeps the proof contract lean:
  - no extra MCP methods beyond `validate/gate/init/catalog/exec`
  - no feature-specific runtime/UI brains in core
  - additive witness/evidence metadata only

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
  - blocks `tier=sunset` (or sunset marker metadata) unless `--allow-sunset`

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
- Keep merge-readiness as a separate proof step via `exec`, not a fourth gate kind.
  The canonical path is `compas.exec merge-truth-check -- --profile <ci|flagship>` once the
  required witness and review artifacts exist.
- Treat plugin installs as **policy changes**; pin registry versions for CI/reproducibility.
- `init --profile ai_first` is the opt-in canonical repo-visible scaffold path:
  `AGENTS.md`, `ARCHITECTURE.md`, `docs/index.md`, `docs/exec-plans/{README,TEMPLATE}.md`,
  and `docs/QUALITY_SCORE.md` with managed markers for later proof plugins.
- Registry recommendations can now match explicit repo signals (for example `ai_first_scaffold`)
  in addition to detected code languages; recommendation flow stays advisory-only.
- The intended split is:
  - worker proof: `validate` + `gate`
  - review/audit proof: canonical review artifacts under `.agents/mcp/compas/reviews/`
  - merge truth: repo-local `merge-truth-check` artifact assembled from the existing proof set
