## Slice-7 — rename tool to compas (brand + config root)

### Goal
Переименовать инструмент в `compas` на пользовательском уровне и закрепить
единственный runtime-конфиг root: `.agents/mcp/compas/*`.

### Scope (touchpoints)
- `.agents/mcp/compas/**` (new canonical config root)
- старый конфиг-root удалён
- `crates/ai-dx-mcp/src/repo.rs` (plugins path)
- `crates/ai-dx-mcp/src/exceptions.rs` (allowlist path)
- `crates/ai-dx-mcp/src/witness.rs` (witness path)
- `crates/ai-dx-mcp/src/server.rs` (tool descriptions/instructions branding)
- `crates/ai-dx-mcp/src/checks/{loc,env_registry}.rs` + tests path fixtures
- `.agents/skills/SKILLS.md`, `.agents/skills/compas-repo/SKILL.md`
- `README.md`, `.gitignore`, `dx`
- `docs/plans/mcp-compas-ai-dx/PLAN.md`

### Non-goals
- Переименование Rust package/crate `ai-dx-mcp`.
- Переименование env var names `AI_DX_*` (вне этого среза).

### Acceptance
- Canonical config root: `.agents/mcp/compas/*`.
- Fallback удалён; используется только `.agents/mcp/compas/*`.
- MCP server user-facing descriptions используют `compas`.
- Validate/gate зелёные на текущем репо после миграции.

### Tests / Verify
- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`
- `./dx ci-fast --dry-run`

### Blockers
- нет

### BranchMind
- Workspace: 1
- Task: TASK-013
- Step: s:0

### Proof
- CMD: `cargo test -p ai-dx-mcp`
- CMD: `cargo run -p ai-dx-mcp -- validate ratchet`
- CMD: `./dx ci-fast --dry-run`
- FILE: `.agents/mcp/compas/plugins/default/plugin.toml`
- FILE: `crates/ai-dx-mcp/src/repo.rs`
- FILE: `crates/ai-dx-mcp/src/exceptions.rs`
- FILE: `docs/plans/mcp-compas-ai-dx/Slice-7.md`

### Result
- Status: PASS (2026-02-14)
- Commit: see `git log --oneline -1`
