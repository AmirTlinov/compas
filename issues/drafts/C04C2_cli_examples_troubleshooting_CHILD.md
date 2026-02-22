# [C04] CLI help + troubleshooting (error messages, examples)

## Concrete scope
Improve UX:
- help text updated for manifest-based registry
- errors explain how to fix (missing full build, missing signature, denied tier, etc.)
- add `plugins help` examples

## Interface boundary
- CLI text + structured error output only.

## Implementation steps
1) Audit all plugin manager errors; ensure they contain:
   - what failed
   - which source/path
   - next action
2) Add a short troubleshooting section in docs.

## Test checklist
- Smoke-run `compas_mcp plugins --help` output contains new examples.

## Definition of Done
- Users don’t need to read code to resolve common failures.

Parent: #46
