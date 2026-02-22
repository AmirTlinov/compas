# Compas plugins (native registry v1)

This document describes **how to consume community plugins** with compas' **native plugin manager**.

Compas plugins are **repo-side configuration + adapters** that live under:

- `.agents/mcp/compas/plugins/<plugin-id>/plugin.toml`

The recommended way to install and update plugins is via the signed community registry:

- `https://github.com/AmirTlinov/compas-plugin-registry`

## Goals

- **One-command install** of a chosen subset of plugins (or a curated pack).
- **Fail-closed trust**: signature verification + sha256 verification of the registry release archive.
- **Deterministic state**: installed files are tracked by a lockfile; drift is detected.
- **Safe extraction**: no system `tar`, strict tar entry validation, bounded resource usage.
- **Governance policy**: experimental/deprecated tiers are blocked unless explicitly allowed.

## Model

Compas consumes a **signed registry manifest** (`registry.manifest.v1.json`) that declares:

- the release archive file name and its sha256
- a list of plugin records (id, path inside archive, governance metadata)
- a list of packs (curated plugin sets)

Compas then:

1) verifies the manifest signature (unless `--allow-unsigned` is provided),
2) downloads/copies the archive next to the manifest (or from the same release base URL),
3) verifies the archive sha256,
4) extracts the archive with a **safe extractor**,
5) installs selected plugins into the target repo and writes a lockfile.

## Trust model (signature verification)

By default, compas ships an embedded public key for the **official community registry**.

### Recommended (production) usage

- Do **not** use `--allow-unsigned`.
- Let compas verify `registry.manifest.v1.json.sig` automatically.

### Dev / local registries

For local testing (custom registry fork / local manifest):

- Use `--pubkey <path-to-pubkey.pem>` to provide a trusted key explicitly, or
- Use `--allow-unsigned` for **non-production** workflows only.

If signature verification fails at any step, compas rejects the install/update path.

## Governance policy (tiers)

Registry plugins have a governance tier:

- `community` (default / safe)
- `experimental` (blocked by default)
- `deprecated` (blocked by default)
- `certified` (reserved for future use)

Native registry installs/updates enforce:

- `tier=experimental` requires `--allow-experimental`
- `tier=deprecated` (or presence of `deprecated` metadata) requires `--allow-deprecated`

This is intentionally **deny-by-default**: agents tend to “try random plugins” unless blocked.

## Commands (CLI)

All commands use the default registry unless `--registry <source>` is provided.

### Discovery

- List plugins:
  - `ai-dx-mcp plugins list -- --json`
- List packs:
  - `ai-dx-mcp plugins packs -- --json`
- Inspect a plugin record:
  - `ai-dx-mcp plugins info spec-adr-gate`

### Install

- Install one plugin:
  - `ai-dx-mcp plugins install --repo-root . -- --plugins spec-adr-gate --force`
- Install a pack:
  - `ai-dx-mcp plugins install --repo-root . -- --packs ai-only-core --force`

Notes:
- `--force` is required when the repo already has unmanaged plugin directories or drift.
- Use `--dry-run` to preview targets without writing.

### Update

- Update previously installed plugins (infers targets from lockfile):
  - `ai-dx-mcp plugins update --repo-root . -- --force`
- Update explicit set:
  - `ai-dx-mcp plugins update --repo-root . -- --plugins spec-adr-gate --force`

### Uninstall

- Uninstall explicit set:
  - `ai-dx-mcp plugins uninstall --repo-root . -- --plugins spec-adr-gate --force`

### Doctor

- Diagnose state and drift:
  - `ai-dx-mcp plugins doctor --repo-root . -- --json`

## State and drift

Compas stores **deterministic installation state**:

- Lockfile: `.agents/mcp/compas/plugins.lock.json`
  - declares installed plugins/packs and the sha256 for each installed file
- Legacy state (migration only): `.agents/mcp/compas/plugins/.registry_state.json`

Drift handling:

- If installed files were modified (or unexpected files appear), compas blocks install/update/uninstall.
- Use `--force` to re-apply from the registry state when you intentionally want to overwrite drift.

## Failure modes and recovery (fail-closed)

Common blockers:

- **Signature failure**: manifest was tampered or you used wrong key.
- **Archive sha mismatch**: the downloaded archive does not match the manifest.
- **Unsafe tar entries**: symlinks, unsupported entry types, path traversal (`..`), multiple top-level roots.
- **Repo drift**: installed files changed; unmanaged plugin dirs exist.
- **Lock contention**: `.agents/mcp/compas/plugins.lock` is held by another process.

Recovery guidance:

- Prefer fixing the root cause (wrong registry, stale cache, manual edits).
- If you need to proceed deterministically, use `--force` and rerun.
- For CI: pin your registry manifest version (not `latest`) and keep `--allow-unsigned` off.

## Notes for AI-only workflows

- Treat plugins as **policy and evidence**, not as “nice-to-have tooling”.
- Keep `ci_fast` lean: fast gates reduce agent “try random things” behavior.
- Keep slow tools in `ci`/`flagship` or scheduled jobs; use compas `impact rules` to avoid wasting time.

