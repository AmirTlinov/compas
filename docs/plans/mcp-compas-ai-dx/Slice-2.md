## Slice-2 — practical LOC ratchet + allowlist exceptions + gate witness

### Goal
Сделать `compas` пригодным к ежедневной работе агента:
- LOC ratchet не блокирует “нормальный рост” под лимитом,
- исключения (allowlist) — единый легальный обход правил с expiry,
- `gate` умеет писать **witness JSON** артефакт (для доказуемости и аудита),
- CLI поддерживает `--gate` (в дополнение к `--validate`).

### Scope (touchpoints)
- `crates/ai-dx-mcp/src/checks/loc.rs`
- `crates/ai-dx-mcp/src/exceptions.rs`
- `crates/ai-dx-mcp/src/api.rs`
- `crates/ai-dx-mcp/src/app.rs`
- `crates/ai-dx-mcp/src/server.rs`
- `crates/ai-dx-mcp/src/main.rs`
- `.agents/mcp/compas/allowlist.toml`
- `.gitignore` (игнор witness)
- `docs/plans/mcp-compas-ai-dx/*`

### Non-goals
- FUNC/CC метрики.
- boundary/env-registry/public-surface checks.

### Acceptance
- LOC ratchet semantics:
  - strict: любое `loc > max_loc` => `loc.max_exceeded`.
  - ratchet:
    - существующий oversized offender (baseline_loc > max_loc) допускается без роста (не падаем на `loc.max_exceeded`),
    - но дальнейший рост oversized offender => `loc.ratchet_regression`,
    - новые offenders => `loc.max_exceeded`,
    - рост под max_loc разрешён.
- Exception protocol:
  - файл `.agents/mcp/compas/allowlist.toml` поддерживает `[[exceptions]]` с `id/rule/path/owner/reason/expires_at?`.
  - невалидный allowlist => `exception.allowlist_invalid` (fail-closed, suppression не применяется).
  - просроченные исключения => `exception.expired` (не подавляется).
  - `ValidateOutput` содержит `suppressed` (подавленные нарушения), и `ok` считается по **unsuppressed**.
- Gate witness:
  - `GateRequest.write_witness=true` пишет `.agents/mcp/compas/witness/gate_<kind>.json`.
  - при ошибке записи => `ApiError.code = witness.write_failed`.
- CLI:
  - `--gate <ci-fast|ci|flagship> [--dry-run] [--write-witness] [--repo-root <path>]` печатает `GateOutput` JSON.

### Tests / Verify
- `cargo test`
- witness smoke:
  - `cargo run -p ai-dx-mcp -- gate ci-fast --write-witness`
  - ожидаем `witness_path` в JSON и файл по этому пути.

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-003
- Step: s:0

### Proof (заполнить при закрытии)
- CMD: `cargo test`
- CMD: `cargo run -p ai-dx-mcp -- gate ci-fast --write-witness`
- FILE: `.agents/mcp/compas/witness/gate_ci-fast.json`
- FILE: `docs/plans/mcp-compas-ai-dx/Slice-2.md`

### Result
- Status: PASS (2026-02-14)
- Commit: `4ea3d64`
