# [C03] Enforce tier policy during install/update

## Concrete scope
During plugin resolution:
- filter plugins by allowed tiers
- fail with actionable message listing denied plugins and their tiers

## Interface boundary
- Pure resolution logic; no filesystem writes.

## Implementation steps
1) Add `Tier` enum in manifest model.
2) Add allowed set computed from CLI flags.
3) Apply to:
   - explicit `--plugins`
   - `--packs` expansion
4) Ensure `list/info` always show tier regardless of policy.

## Test checklist
- Install denies experimental by default.
- Install succeeds with `--allow-experimental`.

## Definition of Done
- Tier policy is secure-by-default and easy to override explicitly.

Parent: #42
