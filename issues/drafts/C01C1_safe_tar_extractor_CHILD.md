# [C01] Implement safe tar.gz extractor (Rust-only)

## Concrete scope
Implement `extract_tar_gz_safe(archive_bytes, out_dir)` with rules:
- Reject any entry with:
  - absolute path
  - `..` components
  - non-UTF8? (either reject or lossy-validate; prefer reject)
  - symlink, hardlink, block/char devices, fifo
- Enforce limits:
  - max files count (**MAX_ENTRIES**, default: 20_000)
  - max single file size (**MAX_FILE_BYTES**, default: 10 MiB)
  - max total uncompressed size (**MAX_TOTAL_BYTES**, default: 200 MiB)
  - max path length (**MAX_PATH_BYTES**, default: 512 bytes)

Also enforce **archive root prefix**:
- archive must contain exactly one top-level directory (e.g. `compas_plugins-<version>/...`)
- all entries must be under that prefix; otherwise reject.

## Interface boundary
- Pure filesystem write to provided directory.
- Caller ensures sha256 verified before extraction.

## Implementation steps
1) Use `flate2::read::GzDecoder` + `tar::Archive`.
2) Iterate entries; for each:
   - validate header entry type
   - validate path (components)
   - enforce `MAX_PATH_BYTES` on UTF-8 normalized path string (fail if non-UTF8)
   - enforce single-root-prefix invariant (discover prefix from the first entry, then require it)
   - pre-check size limits using `entry.header().size()`
   - maintain running counters:
     - `entries_seen += 1` (fail if `> MAX_ENTRIES`)
     - `total_unpacked_bytes += entry_size` (fail if `> MAX_TOTAL_BYTES`)
   - create dirs/files with safe perms (e.g. 0o755/0o644) or preserve only if safe.
3) Implement atomic extraction:
   - extract to temp dir, then rename to final cache dir.

## Test checklist
- Create tar fixtures in tests:
  - `../evil`
  - `/abs/path`
  - symlink entry
  - huge size header
- Ensure extractor rejects them.

## Definition of Done
- Safe extractor blocks common archive exploitation paths and is covered by tests.
- Limits are encoded as constants (with a single override point, if we decide to make them configurable later).
- Archive shape is normalized (single top-level dir), reducing downstream path ambiguity.

Parent: #33
