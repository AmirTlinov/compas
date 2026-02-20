## Slice-5 — receipts/witness hardening + dx integration

### Goal
Довести `gate` до более эксплуатационного формата:
- детерминированные receipts с `bytes + sha256` для stdout/stderr,
- witness metadata + ротация файлов,
- единый `./dx` маршрут для `validate/ci-fast/ci/flagship`.

### Scope (touchpoints)
- `crates/ai-dx-mcp/src/api.rs`
- `crates/ai-dx-mcp/src/app.rs`
- `crates/ai-dx-mcp/src/main.rs`
- `crates/ai-dx-mcp/src/runner.rs`
- `crates/ai-dx-mcp/src/witness.rs`
- `crates/ai-dx-mcp/src/hash.rs` (new)
- `crates/ai-dx-mcp/src/lib.rs`
- `crates/ai-dx-mcp/Cargo.toml`
- `.agents/mcp/compas/baselines/public_surface.json`
- `dx` (new)
- `README.md`
- `.agents/skills/compas-repo/SKILL.md`
- `docs/plans/mcp-compas-ai-dx/PLAN.md`

### Non-goals
- Полноценный artifact store для stdout/stderr payload (вне tail + hash).
- Политика ротации по age/TTL (сейчас только count+size лимиты).

### Acceptance
- `Receipt` расширен:
  - `stdout_bytes`, `stderr_bytes`,
  - `stdout_sha256`, `stderr_sha256`.
- `GateOutput` расширен `witness`:
  - `{ path, size_bytes, sha256, rotated_files }`.
- Witness write path:
  - fail-closed на ошибках сериализации/записи/ротации,
  - ротация в `.agents/mcp/compas/witness` (max files = 20, max total size = 2 MiB),
  - текущий witness-файл не удаляется.
- `./dx` поддерживает:
  - `validate [ratchet|strict|warn]`,
  - `gate <ci-fast|ci|flagship>`,
  - шорткаты `ci-fast|ci|flagship`.
- CLI `ai-dx-mcp` для `--validate/--gate` возвращает ненулевой exit code при `ok=false`.

### Tests / Verify
- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`
- `cargo run -p ai-dx-mcp -- gate ci-fast --dry-run`
- `./dx validate ratchet`
- `./dx ci-fast --dry-run --write-witness`

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-010
- Step: s:0

### Proof
- CMD: `cargo test -p ai-dx-mcp`
- CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
- CMD: `cargo run -p ai-dx-mcp -- gate ci-fast --dry-run`
- CMD: `./dx validate ratchet`
- CMD: `./dx ci-fast --dry-run --write-witness`
- FILE: `dx`
- FILE: `docs/plans/mcp-compas-ai-dx/Slice-5.md`

### Result
- Status: PASS (2026-02-14)
- Commit: see `git log --oneline -1`
