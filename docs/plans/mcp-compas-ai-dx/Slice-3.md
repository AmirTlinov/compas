## Slice-3 — env-registry + effective config

### Goal
Сделать управление env-параметрами в `ai-dx` машинно‑проверяемым:
- единый реестр env переменных,
- fail-closed проверки на отсутствующий/битый registry и unregistered usage,
- детерминированный `effective_config` в `validate` output.

### Scope (touchpoints)
- `crates/ai-dx-mcp/src/api.rs`
- `crates/ai-dx-mcp/src/app.rs`
- `crates/ai-dx-mcp/src/config.rs`
- `crates/ai-dx-mcp/src/repo.rs`
- `crates/ai-dx-mcp/src/checks/mod.rs`
- `crates/ai-dx-mcp/src/checks/env_registry.rs` (new)
- `.agents/mcp/compas/plugins/default/plugin.toml`
- `.agents/mcp/compas/env_registry.toml` (new)
- `README.md`
- `docs/plans/mcp-compas-ai-dx/PLAN.md`

### Non-goals
- Boundary/public-surface checks.
- FUNC/CC ratchet.
- Авто-скан env usage по всему коду репо.

### Acceptance
- `[checks.env_registry]` поддерживается в plugin config.
- При отсутствии registry файла: `env.registry_missing`.
- При невалидном registry (read/parse/schema): `env.registry_invalid`.
- `tools[*].env` ключи без записи в registry дают `env.unregistered_usage`.
- `required=true` + no env/default даёт `env.required_missing`.
- `ValidateOutput` содержит `effective_config`:
  - entries с source `env|default|unset`,
  - sensitive values redacted.
- MCP surface не расширен (остаются `validate`, `gate`, `tools.list`, `tools.describe`, `tools.run`).

### Tests / Verify
- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-007
- Step: s:0

### Proof (заполнить при закрытии)
- CMD: `cargo test -p ai-dx-mcp`
- CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
- FILE: `.agents/mcp/compas/env_registry.toml`

### Result
- Status: PASS (2026-02-14)
- Commit: TBD
