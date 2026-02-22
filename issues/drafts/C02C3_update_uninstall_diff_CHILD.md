# [C02] Implement update + uninstall (plan diff, prune dirs)

## Concrete scope
Implement:
- `plugins update`:
  - re-resolve selection (from flags or lockfile)
  - compute new file plan
  - apply diff: add/update/remove
- `plugins uninstall`:
  - remove files owned by selected plugins (or all from lockfile if none specified)
  - prune empty parent dirs under `.agents/mcp/compas/plugins`

## Interface boundary
- Only touches managed plugin files (from lockfile).
- Never deletes user files outside managed roots.

## Implementation steps
1) Represent file plan as `{ path -> {sha256, owners} }`.
2) For update:
   - removed files: delete if unchanged since lockfile sha
   - modified files: fail unless `--force`
3) For uninstall:
   - same safety: fail if file modified unless `--force`
4) Update lockfile accordingly.

## Test checklist
- Update changes lockfile when registry version changes.
- Uninstall removes expected files and leaves other repo files intact.

## Definition of Done
- Lifecycle commands are safe and predictable.

Parent: #37
