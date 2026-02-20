## Slice-4 — boundary + public surface gates

### Goal
Добавить в `ai-dx validate` две архитектурные защиты:
- boundary rules (import/export policy, deny-regex),
- public-surface ratchet/diff (контроль роста публичного API).

### Scope (touchpoints)
- `crates/ai-dx-mcp/src/api.rs`
- `crates/ai-dx-mcp/src/app.rs`
- `crates/ai-dx-mcp/src/config.rs`
- `crates/ai-dx-mcp/src/repo.rs`
- `crates/ai-dx-mcp/src/checks/mod.rs`
- `crates/ai-dx-mcp/src/checks/boundary.rs` (new)
- `crates/ai-dx-mcp/src/checks/public_surface.rs` (new)
- `crates/ai-dx-mcp/src/witness.rs` (new, split to keep LOC under limit)
- `crates/ai-dx-mcp/tests/boundary_check.rs` (new)
- `crates/ai-dx-mcp/tests/public_surface_check.rs` (new)
- `.agents/mcp/compas/plugins/default/plugin.toml`
- `.agents/mcp/compas/baselines/public_surface.json` (new)
- `README.md`
- `docs/plans/mcp-compas-ai-dx/PLAN.md`

### Non-goals
- Семантический AST-анализ зависимостей между bounded contexts.
- Полноценный semver-парсер публичного API.

### Acceptance
- Boundary:
  - `[checks.boundary]` + `[[checks.boundary.rules]]` поддерживаются в plugin config.
  - violation code: `boundary.rule_violation`.
  - invalid config/regex -> `boundary.check_failed` (fail-closed).
- Public surface:
  - `[checks.public_surface]` поддерживается.
  - `public_surface.max_exceeded` для абсолютного лимита.
  - `public_surface.ratchet_regression` при росте public items vs baseline.
  - baseline path: `.agents/mcp/compas/baselines/public_surface.json`.
- Validate output содержит summaries:
  - `boundary { files_scanned, rules_checked, violations }`
  - `public_surface { baseline_path, max_pub_items, items_total, added_vs_baseline, removed_vs_baseline }`

### Tests / Verify
- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`
- `cargo run -p ai-dx-mcp -- gate ci-fast --dry-run`

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-008
- Step: s:0

### Proof
- CMD: `cargo test -p ai-dx-mcp`
- CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
- CMD: `cargo run -p ai-dx-mcp -- gate ci-fast --dry-run`
- FILE: `.agents/mcp/compas/baselines/public_surface.json`
- FILE: `docs/plans/mcp-compas-ai-dx/Slice-4.md`

### Result
- Status: PASS (2026-02-14)
- Commit: TBD
