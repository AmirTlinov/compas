---
name: compas-repo
description: "Локальный профиль compas для этого репозитория: verify/gate инварианты и repo-specific DX rails."
Last verified: 2026-02-16
status: PASS
---

# Skill: compas-repo

[LEGEND]
PLUGIN = Repo-side конфиг для tools/checks (`.agents/mcp/compas/plugins/*/plugin.toml`).
RATCHET = Режим, где качество нельзя ухудшать относительно baseline.
BASELINE = Файлы-эталоны (`.agents/mcp/compas/baselines/*`).
ALLOWLIST = Exception protocol (`.agents/mcp/compas/allowlist.toml`).
ENV_REGISTRY = Единый реестр env vars (`.agents/mcp/compas/env_registry.toml`).
EFFECTIVE_CONFIG = Итоговая env-конфигурация в `ValidateOutput`.
WITNESS = JSON-артефакт gate (`.agents/mcp/compas/witness/*`).
ENV_DEFAULTS = Env-дефолты для CLI/MCP когда явные параметры не переданы.

[CONTENT]
## ALPHA-IRON RULE
- Никаких устаревших хвостов и режимов совместимости.
- Breaking changes допустимы и ожидаемы.
- История — только в git; не держим «исторические» комментарии в коде/рабочих доках.

## Когда использовать
- Перед закрытием любого SLICE: запускать `validate` (и при необходимости `gate`) как обязательный Verify.

## Repo Verify invariant (2026-02-16)
- Для этого репозитория каноничный Verify стек:  
  `cargo test -p ai-dx-mcp` → `cargo run -p ai-dx-mcp -- validate ratchet` → `./dx ci-fast --dry-run`.
- В plan/docs не использовать абстрактный `make check`: только реальные команды из AGENTS.md.

## Что даёт сейчас
- MCP tools: `compas.validate`, `compas.gate`, `compas.init`, `compas.catalog`, `compas.exec`.
- `gate.kind`: канонично `ci_fast|ci|flagship` + UX-алиас `ci-fast`.
- `compas.init`: builtin packs → `tools/custom/*/tool.toml` + `.agents/mcp/compas/packs/*` + `packs.lock` + minimal gate wiring.
- CLI bootstrap: `init` (dry-run по умолчанию) + `--apply` для записи файлов (аналог `compas.init`).
- Plugin/tool manifests enforce strict schema (id pattern + mandatory concise descriptions + no empty plugin payload).
- Tool execution policy is fail-closed by default (`[tool_policy].mode="allowlist"`):
  command basename must be in built-in allowlist or in `[tool_policy].allow_commands`.
- `compas.catalog` включает `plugin_id` в tools-выборке — видно владельца инструмента без чтения исходников.
- `validate` теперь отдаёт CIM foundation поля:
  - `findings_v2` (normalised finding envelope с `severity/category/confidence/fix_recipe`),
  - `risk_summary` (агрегаты по category/severity),
  - `coverage` (покрытие canonical failure-mode catalog из `.agents/mcp/compas/failure_modes.toml`),
  - `trust_score` (score/grade по findings).
- Failure-mode catalog расширяется fail-closed через `.agents/mcp/compas/failure_modes.toml`
  (строгий TOML, уникальные mode id, unknown-поля запрещены).
- Архитектурные карты в `ARCHITECTURE.md` и `AGENTS.md` поддерживаются `./dx docs-sync` (`--check` для гейта).
- Enforced checks:
  - LOC ratchet (`loc.max_exceeded`, `loc.ratchet_regression`),
  - ENV registry (`env.registry_*`, `env.unregistered_usage`, `env.required_missing`),
  - Boundary rules (`boundary.rule_violation`, `boundary.check_failed`),
  - High-impact runtime boundary preset (`no-runtime-unwrap-expect`, `no-runtime-panic`, `no-runtime-stdout`),
  - Public surface ratchet (`public_surface.max_exceeded`, `public_surface.ratchet_regression`).
  - Duplicates ratchet (`duplicates.found`, `duplicates.ratchet_regression`).
  - Supply-chain baseline (`supply_chain.lockfile_missing`, `supply_chain.prerelease_dependency`).
  - Anti-bloat budgets (`tool_budget.max_*`).
- `ALLOWLIST` для контролируемых исключений.
- `WITNESS` через `--write-witness` + мета (`path/size/sha256/rotated_files`).
- `Receipt` содержит bounded tails + `stdout/stderr bytes` и `sha256`.
- Gate runner fail-closed invariants: `gate.empty_sequence`, `gate.duplicate_tool_id`, `gate.receipt_invariant_failed`.
- `ENV_DEFAULTS`:
  - `AI_DX_REPO_ROOT`: default `repo_root` для CLI/MCP если параметр не передан.
  - `AI_DX_WRITE_WITNESS`: default для `gate.write_witness` если флаг/поле не переданы (`1|true`).

## Быстрые команды
```bash
cargo run -p ai-dx-mcp -- init --repo-root <path>          # dry-run init plan
cargo run -p ai-dx-mcp -- init --apply --repo-root <path>  # apply init plan
cargo run -p ai-dx-mcp -- validate ratchet --write-baseline
cargo run -p ai-dx-mcp -- validate ratchet
cargo run -p ai-dx-mcp -- gate ci-fast --dry-run
cargo run -p ai-dx-mcp -- gate ci-fast --write-witness
cargo test -p ai-dx-mcp --no-default-features
cargo test -p ai-dx-mcp --no-default-features --features wasm
./dx validate ratchet
./dx init --apply --repo-root <path>
./dx ci-fast --dry-run
./dx flagship --dry-run --write-witness
```

## Build profiles (dist)
- **full (default)**: `cargo build -p ai-dx-mcp` (`default-features`).
- **lite**: `cargo build -p ai-dx-mcp --no-default-features` (fail-closed on external packs).
- **wasm (experimental)**: `cargo test -p ai-dx-mcp --no-default-features --features wasm` (deny-by-default imports; InitPlan-only output).

## Подключение инструментов
1) Добавь/измени `[[tools]]` в plugin.toml
2) Или импортируй tool-файлы через `tool_import_globs = ["tools/custom/**/tool.toml"]`
3) Для custom command добавь basename в `[tool_policy].allow_commands` (или осознанно включи `mode = "allow_any"`)
4) Привяжи `gate.ci_fast|ci|flagship`
5) Настрой checks: `[checks.loc]`, `[checks.env_registry]`, `[checks.boundary]`, `[checks.surface]`, `[checks.duplicates]`, `[checks.supply_chain]`, `[checks.tool_budget]`

## Интерпретация ошибок
- Смотреть стабильные `ApiError.code` и `Violation.code`.
- Если `ok=false`, но операция выполнилась (например, tool non-zero exit), ожидай краткий digest в `error` (`compas.exec.exit_nonzero`, `gate.tool_failed`) и детали в `receipt/receipts`.
- В ratchet baseline должен существовать; инициализировать через `validate ... --write-baseline`.
- Если репо ещё не инициализировано: ошибка `config.plugins_dir_missing` подсказывает ожидаемый путь `.agents/mcp/compas/plugins/*/plugin.toml` и варианты bootstrap (`compas.init` / `init`, или вручную `plugin.toml` + `tool.toml`).
