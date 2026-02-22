# [C01] Download archive + sha256 verify before extraction

## Concrete scope
Implement:
- `download_archive(url) -> bytes` (bounded)
- `sha256(bytes) == manifest.archive.sha256` check (fail-closed)

## Interface boundary
- Network only in `full` build.
- Lite build: archive URL install should fail with clear error; local file install may still work.

## Implementation steps
1) Add size limit (e.g. 50MB compressed).
2) Verify content-type not required; rely on sha.
3) Compute sha while streaming if possible (avoid storing huge bytes twice).
4) If sha mismatch:
   - delete cache entry
   - error message includes expected/actual sha (short) and URL.

## Test checklist
- Positive: known fixture bytes.
- Negative: sha mismatch triggers failure and no extraction occurs.

## Definition of Done
- Archive integrity is checked before touching filesystem extraction.

Parent: #33
