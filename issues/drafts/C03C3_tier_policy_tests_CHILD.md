# [C03] Add tier policy tests (deny-by-default)

## Concrete scope
Add tests to ensure tier policy doesn’t regress:
- default deny experimental/deprecated
- explicit allow works
- pack expansion respects policy

## Interface boundary
- Tests only.

## Implementation steps
1) Add manifest fixtures with mixed tiers.
2) Add integration test that runs `plugins install --dry-run` and checks exit code + stderr.

## Test checklist
- `cargo test` passes.

## Definition of Done
- Tier policy is protected by regression tests.

Parent: #42
