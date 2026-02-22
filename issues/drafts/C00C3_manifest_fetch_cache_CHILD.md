# [C00] Manifest fetch + cache (bounded, deterministic, no stale trust)

## Concrete scope
Implement manifest retrieval for URL sources with:
- bounded download size (e.g. <= 5 MB)
- cache by URL + ETag (optional) or by sha256(manifest_bytes)
- explicit “refresh” semantics (avoid silent stale trust)

## Interface boundary
- Network only in `full` build (feature-gated like existing reqwest usage).
- Lite build must fail with actionable error for URL registries.

## Implementation steps
1) Add cache dir: `$XDG_CACHE_HOME/compas/plugins/manifest/<sha256(url)>/`
2) Store:
   - `manifest.json`
   - `manifest.sig`
   - `meta.json` (url, fetched_at, etag if available)
3) Implement:
   - `--no-cache` (optional)
   - default behavior: reuse cache if fresh OR always revalidate (choose one and document)
4) Ensure cache is never used if signature verification fails.

## Test checklist
- URL mode tests behind feature flag and possibly ignored in CI (prefer local fixtures).
- File mode always tested.

## Definition of Done
- Manifest fetch is safe, bounded, and does not create a silent stale security state.

Parent: #28
