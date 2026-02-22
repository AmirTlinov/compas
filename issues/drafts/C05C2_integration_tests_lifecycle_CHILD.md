# [C05] Integration tests: install/update/uninstall/doctor lifecycle

## Concrete scope
Add integration tests using temp dirs:
- create fake registry cache tree (small)
- create manifest referencing fake archive sha
- run plugin manager commands in `--dry-run` and real modes
- assert lockfile content and filesystem effects

## Interface boundary
- Tests only; should not require network.

## Implementation steps
1) Add helper to create a minimal plugin directory fixture in temp.
2) Ensure tool_import_globs and plugin.toml are consistent.
3) Test flows:
   - install → doctor ok
   - modify file → doctor fails
   - uninstall → files removed

## Test checklist
- `cargo test` passes on Linux CI.

## Definition of Done
- Core lifecycle behavior is protected against regressions.

Parent: #49
