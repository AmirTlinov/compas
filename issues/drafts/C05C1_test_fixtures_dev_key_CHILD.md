# [C05] Add test fixtures: dev keypair + signed manifest bytes

## Concrete scope
Add deterministic fixtures for tests:
- a dev P-256 keypair (NOT the production registry key)
- a signed manifest JSON blob + signature

## Interface boundary
- Fixtures only; production keys live only in registry GH secrets.

## Implementation steps
1) Generate dev keypair in-repo for tests:
   - embed PEM strings in test module OR store under `crates/ai-dx-mcp/tests/fixtures/`
2) Store manifest bytes exactly as signed (avoid reformatting).
3) Store signature base64 string.

## Test checklist
- Signature verification test passes with fixtures.

## Definition of Done
- Plugin manager interop tests do not require network or GH secrets.

Parent: #49
