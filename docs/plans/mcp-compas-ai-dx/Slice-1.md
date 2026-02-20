## Slice-1 — MVP skeleton (`ai-dx` MCP)

### Goal
Сделать “работающий скелет” MCP‑компаса:
- MCP server на Rust (`rmcp`) со стабильными tool‑эндпоинтами,
- repo‑side config/plugins,
- runner с таймаутами и ограничением вывода,
- `validate` с **ratchet‑чеком LOC** (baseline файл) + отчёт.

### Scope (touchpoints)
- `Cargo.toml` (workspace)
- `crates/ai-dx-mcp/**`
- `.agents/mcp/compas/plugins/default/plugin.toml`
- `.agents/mcp/compas/baselines/loc.json` (генерируется `write_baseline=true`)
- `docs/plans/mcp-compas-ai-dx/*`

### Non-goals
- Все остальные checks (env/boundary/public surface/artifacts), WASM‑плагины, CI интеграция в чужие репо.

### Acceptance
- `cargo test` проходит.
- `ai-dx-mcp` запускается (stdio) и объявляет tools: `validate`, `gate`, `tools.list`, `tools.describe`, `tools.run`.
- `validate(mode=ratchet)`:
  - при отсутствии baseline — fail‑closed с понятным сообщением,
  - при `write_baseline=true` — создаёт baseline и выдаёт report.
- `tools.run` реально запускает хотя бы один tool из plugin.toml (для этого репо — `cargo test`) и возвращает receipt (exit_code, duration_ms, stdout_tail/stderr_tail).

### Tests / Verify
- `cargo test`
- smoke: `cargo run -p ai-dx-mcp -- --version` (CLI help/version)

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-001
- Step: s:0

### PR контракт (@codex review)
Проверить:
- Не раздут MCP surface (≤5 tools).
- Runner не использует shell, корректно ограничивает вывод и убивает процесс по таймауту.
- Ratchet semantics: существующие файлы не могут ухудшаться относительно baseline; новые — под абсолютным лимитом.
- Ошибки: есть стабильные error codes; нет `anyhow` наружу; нет silent‑fail.

### Proof (заполнить при закрытии)
- CMD: `cargo test`
- FILE: `docs/plans/mcp-compas-ai-dx/Slice-1.md`

### Result
- Status: PASS (2026-02-14)
- Commit: `49e79fb`
- Extra proof:
  - CMD: `cargo run -p ai-dx-mcp -- validate ratchet --write-baseline`
  - CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
