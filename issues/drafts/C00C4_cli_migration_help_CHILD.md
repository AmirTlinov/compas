# [C00] CLI UX + backward compatibility/migration

## Concrete scope
Update `compas plugins ...` CLI to:
- accept `--registry <source>` where `<source>` can be:
  - URL to manifest json (default)
  - local path to manifest json
  - (temporary) URL/path to old tarball registry (compat mode)
- expose explicit flags instead of “passthrough installer args”:
  - `--plugins <csv>`
  - `--packs <csv>`
  - `--dry-run`
  - `--allow-experimental` (later, C03)

## Interface boundary
- Pure CLI + parsing + dispatch. No tar extraction here.

## Implementation steps
1) Update help text in `crates/ai-dx-mcp/src/cli.rs`:
   - new defaults
   - examples
2) Keep compatibility:
   - define **RegistrySource v2** auto-detect contract (fail-closed on ambiguity):
     - if value is URL ending with `.json` → manifest URL
     - else if value is URL ending with `.tar.gz`/`.tgz` → legacy archive URL (compat)
     - else if value is local file and first non-ws byte is `{` → manifest file
     - else if value is local file ending with `.tar.gz`/`.tgz` → legacy archive file (compat)
     - else if value is local dir containing `scripts/compas_plugins.py` → legacy registry checkout (compat)
     - otherwise: error “unknown registry source type; expected manifest json or legacy registry”
   - detect if `--registry` points to `.tar.gz` / directory containing `scripts/compas_plugins.py`
   - print deprecation warning and either:
     - refuse in strict mode, or
     - fallback to legacy installer path (temporary)
3) Ensure output remains JSON for automation.
4) Document lite/full build behavior:
   - lite build: URL sources are not supported (clear error with next action)
   - full build: URL sources supported

## Test checklist
- CLI parsing tests updated.
- Legacy mode still works (if kept).

## Definition of Done
- Users can install plugins without needing to know registry internal scripts.

Parent: #28
