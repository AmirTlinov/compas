# [C04] Docs: plugin install guide + trust model (compas side)

## Concrete scope
Add documentation in compas repo:
- where registry comes from (manifest URL)
- what is verified (signature + sha)
- how to install packs/plugins
- how lockfile works

## Interface boundary
- Docs only.

## Implementation steps
1) Update README or create `docs/PLUGINS.md`.
2) Link from `AGENTS.md` to avoid agent confusion.

## Test checklist
- N/A.

## Definition of Done
- Plugin manager is understandable to new users and contributors.

Parent: #46
