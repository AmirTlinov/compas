# Compas: plugin install lifecycle + lockfile v1 (install/update/uninstall/doctor)

## Outcome
Make plugin installs reproducible and auditable:
- compas installs selected plugins into the target repo
- writes a lockfile with exact versions + files + hashes
- supports update/uninstall/doctor deterministically

## Scope IN
- Lockfile: `.agents/mcp/compas/plugins.lock.json` (schema v1).
- Cross-process lock:
  - plugin lifecycle commands acquire an exclusive lock (prevents concurrent install/update/uninstall/doctor races)
- Install:
  - resolve plugins/packs from manifest
  - copy files from cached registry tree into target repo
  - atomic apply (temp → rename)
- Update:
  - re-resolve selection
  - compute diff (add/remove/update files)
- Uninstall:
  - remove files owned by selected plugins
  - prune empty dirs
- Doctor:
  - verify that all locked files exist and match hashes
  - detect unmanaged plugin dirs
- Migration:
  - read legacy `.registry_state.json` and offer conversion.

## Scope OUT
- Running compas gate automatically after install (optional follow-up).
- Multi-registry composition (single registry for now).

## Contracts touched
- New: `docs/PLUGIN_LOCKFILE_V1.md`
- CLI output JSON contract for plugins commands.

## Deliverables
- Working `compas plugins install|update|uninstall|doctor` without Python.
- Lockfile schema + docs.
- Tests for lockfile and file ownership.

## Dependencies
- Safe registry cache tree (C01).
- Manifest parsing (C00).

## Risks
- Removing wrong files → must be ownership-based and fail-closed on ambiguity.
- Windows path semantics (if supported) → normalize paths.
- Races: `plugins install/update` vs `gate/validate` or two installs in parallel → must be prevented by a lock and/or by “read snapshot” semantics.

## Acceptance criteria
- Install writes lockfile with plugin ids and per-file sha256.
- Doctor fails if any locked file missing/modified.
- Uninstall removes only files tracked in lockfile.
- Two concurrent plugin operations cannot corrupt state (either serialize via lock or fail fast with an actionable error).

## Required tests
- Integration test with temp repo:
  - install plugin
  - mutate file → doctor fails
  - uninstall → file removed

## Non-goals
- Perfect merge conflict handling; we fail-closed if target files were modified outside compas ownership.

## Child issues
_Populated by issue publisher._

## Child issues
- [ ] #38 — C02C1 — Lockfile v1 schema + read/write
- [ ] #39 — C02C2 — Install: atomic apply + hashing + conflict detection
- [ ] #40 — C02C3 — Update + uninstall diff (safe removals)
- [ ] #41 — C02C4 — Doctor + legacy migration (.registry_state.json → lockfile)
