# [C02] Define lockfile v1 schema + read/write

## Concrete scope
Introduce `.agents/mcp/compas/plugins.lock.json` with schema v1:
- `schema`: `"compas.plugins.lock.v1"`
- `registry`: `{ url?, manifest_sha256, manifest_version }`
- `selection`: `{ plugins[], packs[] }`
- `installed_at` optional (can omit for determinism; or include but do not sign)
- `files[]`: list of `{ path, sha256, plugin_ids[] }` (ownership)

## Interface boundary
- Lockfile is the SSOT for uninstall/doctor.
- All paths are repo-relative, forward-slash normalized.

## Implementation steps
1) Define Rust structs (serde).
2) Implement load/save with:
   - stable ordering
   - pretty JSON output for humans
3) Add helper:
   - compute sha256 of file
   - normalize paths

## Test checklist
- Roundtrip read/write stable.
- Reject invalid schema/version.

## Definition of Done
- Lockfile v1 is implemented and documented.

Parent: #37
