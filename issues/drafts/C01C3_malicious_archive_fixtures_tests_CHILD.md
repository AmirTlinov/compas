# [C01] Add malicious archive fixtures/tests (regression suite)

## Concrete scope
Add a regression suite for archive safety:
- generate malicious tarballs in tests (no binary fixtures committed)
- ensure extractor rejects each case with a specific error code/message class

## Interface boundary
- Tests only.

## Implementation steps
1) Add helper to build tar in-memory with `tar::Builder`.
2) Cases:
   - traversal `../`
   - absolute path
   - symlink
   - hardlink
   - oversized header sizes
   - too many files
3) Assert:
   - extraction returns error
   - output dir remains empty or temp dir deleted

## Test checklist
- `cargo test -p ai-dx-mcp` covers suite.

## Definition of Done
- Archive safety is protected against regressions.

Parent: #33
