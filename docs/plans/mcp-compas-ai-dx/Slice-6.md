## Slice-6 — tool imports via repo manifests

### Goal
Сделать подключение project tools менее хрупким: `plugin.toml` должен поддерживать
импорт `tool.toml` по glob (`tool_import_globs`) без ручного дублирования `[[tools]]`.

### Scope (touchpoints)
- `crates/ai-dx-mcp/src/config.rs`
- `crates/ai-dx-mcp/src/repo.rs`
- `crates/ai-dx-mcp/Cargo.toml`
- `crates/ai-dx-mcp/tests/repo_imports.rs` (new)
- `.agents/mcp/compas/plugins/default/plugin.toml`
- `tools/custom/cargo-test/tool.toml` (new)
- `README.md`
- `.agents/skills/compas-repo/SKILL.md`
- `docs/plans/mcp-compas-ai-dx/PLAN.md`

### Non-goals
- Многоуровневая композиция импортов (imports of imports).
- Приоритет/override правил между несколькими tool файлами с одинаковым id (вместо этого fail-closed).

### Acceptance
- `PluginConfig` поддерживает `tool_import_globs: Vec<String>`.
- `load_repo_config` загружает `tool.toml` по glob и мерджит с inline tools.
- Конфликт id между inline/imported (или imported/imported) -> `config.duplicate_tool_id`.
- Ошибки glob/read/parse imported tool дают явные коды `config.import_*`.
- Default repo plugin использует import glob вместо inline `[[tools]]`.

### Tests / Verify
- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`
- `./dx ci-fast --dry-run`

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-012
- Step: s:0

### Proof
- CMD: `cargo test -p ai-dx-mcp`
- CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
- CMD: `./dx ci-fast --dry-run`
- FILE: `.agents/mcp/compas/plugins/default/plugin.toml`
- FILE: `tools/custom/cargo-test/tool.toml`
- FILE: `crates/ai-dx-mcp/tests/repo_imports.rs`

### Result
- Status: PASS (2026-02-14)
- Commit: see `git log --oneline -1`
