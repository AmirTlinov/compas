# [C02] Implement install (atomic apply, hashing, conflict detection)

## Concrete scope
Implement `plugins install` to:
- resolve plugin file set from registry cache tree
- copy to target repo under `.agents/mcp/compas/plugins/<id>/...`
- compute sha256 for each installed file
- write lockfile v1

Must detect and fail on conflicts:
- target file exists but is not owned by compas (no lockfile entry) → fail (unless `--force`)
- lockfile exists but indicates different ownership and file differs → fail-closed

## Interface boundary
- Inputs: manifest, cached registry tree, target repo root, selection.
- Outputs: files on disk + lockfile.

## Implementation steps
0) Acquire an exclusive cross-process lock (e.g. `./.agents/mcp/compas/plugins.lock.json.lock`):
   - if lock is held, fail fast with a message: “another compas plugins operation is running”.
1) Compute install plan:
   - for each selected plugin id, include its plugin dir contents
   - include imported tools referenced by plugin.toml `tool_import_globs`
   - include script/data referenced by tool args (strict: only inside plugin dir)
2) Copy using atomic staging:
   - write into temp dir under repo root
   - fsync optional
   - rename into place
3) Hash each copied file and record ownership.

## Test checklist
- Install into empty repo works.
- Installing twice is idempotent.
- If user edits installed file, re-install fails unless `--force`.

## Definition of Done
- Install is deterministic, safe, and produces lockfile.

Parent: #37
