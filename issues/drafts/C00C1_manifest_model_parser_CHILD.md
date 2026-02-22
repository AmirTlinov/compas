# [C00] Manifest v1 model + parsing + source resolution

## Concrete scope
Implement Rust types and parsing for registry manifest v1:
- strict JSON parse (deny unknown fields where reasonable)
- stable behavior on missing/optional fields

Support registry sources:
- URL to manifest JSON
- local file path to manifest JSON
- local registry checkout dir (dev mode) → build manifest on the fly is OUT; instead allow `--registry-dir` only in later slice OR require `--registry` points to a built manifest file.

## Interface boundary
- Input: bytes of manifest JSON.
- Output: typed `RegistryManifestV1` with validated invariants.

## Implementation steps
1) Add module (e.g. `crates/ai-dx-mcp/src/plugins/manifest_v1.rs`):
   - `RegistryManifestV1`
   - `PluginRecordV1`
   - `PackRecordV1`
2) Add invariant validation:
   - ids regex
   - unique plugin ids + aliases
   - packs reference known plugins
   - archive sha256 is 64 hex chars
3) Add a minimal “resolver” that returns:
   - `manifest_bytes`
   - `manifest_url` (optional)
   - `cache_key`

## Test checklist
- Parse known-good fixture.
- Reject duplicate ids, bad sha, unknown pack refs.

## Definition of Done
- Manifest parsing is stable and fail-closed.

Parent: #28
