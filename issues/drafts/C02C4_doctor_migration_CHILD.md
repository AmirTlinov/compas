# [C02] Doctor command + legacy migration (.registry_state.json → lockfile)

## Concrete scope
Implement:
- `plugins doctor`:
  - validates lockfile exists
  - checks every file exists and sha matches
  - detects unknown plugin dirs/files under `.agents/mcp/compas/plugins` not in lockfile
- Migration:
  - if legacy `.agents/mcp/compas/plugins/.registry_state.json` exists:
    - show warning
    - provide `plugins migrate` OR implicit conversion on install/update with `--migrate`

## Interface boundary
- Read-only by default.
- Any mutation (migration) must be explicit or guarded by flag.

## Implementation steps
1) Implement doctor result JSON:
   - `ok`
   - `missing_files[]`
   - `modified_files[]`
   - `unknown_files[]`
2) Implement migration logic:
   - parse legacy state json
   - reconstruct selection + files list if possible
   - write lockfile with “unknown sha” entries OR recompute sha from disk

## Test checklist
- Doctor detects modified file.
- Migration produces a lockfile and then doctor works.

## Definition of Done
- Users can recover from legacy installs and have a single SSOT.

Parent: #37
