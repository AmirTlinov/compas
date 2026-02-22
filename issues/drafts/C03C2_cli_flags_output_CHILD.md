# [C03] CLI flags + JSON output for tier/maintainers/tags

## Concrete scope
Expose metadata in compas CLI:
- `plugins list --json` includes: `tier`, `maintainers`, `tags`, `compat`.
- `plugins info <id>` includes the same + resolved file plan preview.
- Add flags:
  - `--allow-experimental`
  - `--allow-deprecated`

## Interface boundary
- CLI + output only.

## Implementation steps
1) Extend CLI parsing (keep backward compatibility where possible).
2) Extend JSON output structs and tests.

## Test checklist
- Snapshot-style test for JSON output keys presence.

## Definition of Done
- Metadata is visible to users and automation.

Parent: #42
