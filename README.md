# compas (MCP quality compass) — MVP

Единая точка входа для агентов:
- **compas.validate**: ratchet/strict/warn (LOC + env-registry + boundary + public-surface + duplicates),
- **compas.gate**: validate(ratchet) + запуск цепочки project-tools,
- **compas.init** (MCP): bootstrap repo (builtin packs → tools + plugin.toml + packs.lock),
- **compas.catalog**: единая точка просмотра plugin/tool каталога,
- **compas.exec**: запуск tool_id с receipt/witness.

## ALPHA-IRON RULE
- Никаких устаревших хвостов и режимов совместимости в коде и рабочих доках.
- Breaking changes разрешены по умолчанию (этап глубокой альфы, пользователей нет).
- История решений хранится в git/PR, а не в комментариях и «исторических» примечаниях в кодовой базе.

## Repo-side конфиг
- Плагины: `.agents/mcp/compas/plugins/*/plugin.toml`
- Импорт tool-манифестов: `tool_import_globs = ["tools/custom/**/tool.toml"]`
- Packs (init): `.agents/mcp/compas/packs/*/pack.toml` + `.agents/mcp/compas/packs.lock`
- Baselines: `.agents/mcp/compas/baselines/*.json`
- Allowlist (exceptions): `.agents/mcp/compas/allowlist.toml`
- Env registry: `.agents/mcp/compas/env_registry.toml`
- Witness (runtime артефакты): `.agents/mcp/compas/witness/` (ignored by git)
- Архитектурные доки (auto-sync): `ARCHITECTURE.md`, `AGENTS.md`

## Строгий стандарт plugin/tool манифестов
- `plugin.id` и `tool.id`: `^[a-z0-9][a-z0-9_-]{1,63}$`.
- `plugin.description` и `tool.description`: обязательны, 12..220 символов.
- Неизвестные поля в `plugin.toml` и `tool.toml` запрещены (`deny_unknown_fields`).
- `tool.command` обязателен и не может быть пустым.
- По умолчанию действует execution policy (`[tool_policy].mode="allowlist"`):
  команда инструмента должна входить в безопасный allowlist (встроенный + `[tool_policy].allow_commands`).
  Для явно доверенного репо можно включить `[tool_policy].mode="allow_any"`.
- Плагин не может быть “пустышкой”: нужен хотя бы один из payload-элементов
  (tools/imports/checks/gate).
- Любой `gate` tool id должен существовать, иначе fail-closed (`config.unknown_gate_tool`).
- Дубли `plugin.id`/`tool.id` запрещены (fail-closed).

## Локальный quickstart (CLI)
Инициализировать baseline (1 раз на репо):
```bash
cargo run -p ai-dx-mcp -- validate ratchet --write-baseline
```

Валидация:
```bash
cargo run -p ai-dx-mcp -- validate ratchet
```

Gate:
```bash
cargo run -p ai-dx-mcp -- gate ci-fast
cargo run -p ai-dx-mcp -- gate ci-fast --dry-run
cargo run -p ai-dx-mcp -- gate ci-fast --write-witness
```

Gate-invariants (fail-closed):
- `gate.empty_sequence` — выбранный gate-kind не содержит инструментов.
- `gate.duplicate_tool_id` — дубликат tool id в последовательности gate.
- `gate.receipt_invariant_failed` — receipt без обязательного failure/sha context.

Через repo‑wrapper (`./dx`) те же маршруты:
```bash
./dx validate ratchet
./dx ci-fast --dry-run
./dx flagship --dry-run --write-witness
./dx docs-sync
./dx docs-sync --check
```

CLI возвращает ненулевой exit code, если `validate/gate` завершились с `ok=false`.

## Проверки validate

### LOC ratchet
- strict: `loc > max_loc` → `loc.max_exceeded`.
- ratchet: уже существующие offenders могут быть подавлены allowlist; новые регрессии блокируются.

### Env registry + effective config
- `[checks.env_registry]` читает `.agents/mcp/compas/env_registry.toml`.
- `tools[*].env` без записи в registry → `env.unregistered_usage`.
- `required=true` без env/default → `env.required_missing`.
- Невалидный registry → `env.registry_invalid`.
- `ValidateOutput.effective_config` показывает source=`env|default|unset`, sensitive значения редактируются.

### Boundary rules (import/export)
- `[checks.boundary]` + `[[checks.boundary.rules]]` с `deny_regex`.
- Match правила → `boundary.rule_violation`.
- Невалидный regex/конфиг → `boundary.check_failed` (fail-closed).

### Public surface diff/ratchet
- `[checks.surface]` считает публичные элементы (`pub mod/use/fn/struct/enum/trait/const/static/type`).
- Absolute gate: `items_total > max_pub_items` → `public_surface.max_exceeded`.
- Ratchet gate: новые публичные элементы vs baseline → `public_surface.ratchet_regression`.
- Baseline файл: `.agents/mcp/compas/baselines/public_surface.json`.

### Duplicates (identical files) + ratchet
- `[checks.duplicates]` находит идентичные файлы по sha256 (bounded `max_file_bytes`).
- Strict: `duplicates.found`.
- Ratchet: `duplicates.ratchet_regression` (новые группы/расширения vs baseline).

### Supply-chain baseline
- `[checks.supply_chain]` fail-closed проверяет минимальную гигиену lockfiles:
  - Rust manifests ⇒ нужен `Cargo.lock`,
  - Node manifests ⇒ нужен lockfile (`package-lock.json` / `pnpm-lock.yaml` / `yarn.lock` / `bun.lock*`),
  - Python manifests ⇒ нужен lockfile (`poetry.lock` / `uv.lock` / `Pipfile.lock` / `requirements.txt`).
- Дополнительно блокирует prerelease зависимости в `Cargo.toml` и `package.json`:
  - `supply_chain.prerelease_dependency`.

### Anti-bloat governance (tool/check budgets)
- `[checks.tool_budget]` fail-closed ограничивает сложность агентского DX:
  - `max_tools_total`,
  - `max_tools_per_plugin`,
  - `max_gate_tools_per_kind`,
  - `max_checks_total`.
- Нарушения:
  - `tool_budget.max_tools_total_exceeded`,
  - `tool_budget.max_tools_per_plugin_exceeded`,
  - `tool_budget.max_gate_tools_exceeded`,
  - `tool_budget.max_checks_total_exceeded`.

### High-impact runtime boundary presets
- В default plugin добавлен `boundary-high-impact-runtime-rust` (fail-closed).
- Он проверяет runtime-путь на:
  - `unwrap/expect` (`rule_id=no-runtime-unwrap-expect`),
  - `panic!` (`rule_id=no-runtime-panic`),
  - `println!/eprintln!` (`rule_id=no-runtime-stdout`).
- Для Rust учитывается `strip_rust_cfg_test_blocks=true`: `#[cfg(test)] mod ...` не шумит в runtime-гейте.

## Exception protocol (allowlist)
- Файл: `.agents/mcp/compas/allowlist.toml`.
- Формат: `[[exceptions]]` с `id, rule, path, owner, reason, expires_at`.
- `path` — точный относительный путь (globs запрещены).
- Невалидный allowlist → `exception.allowlist_invalid` (suppression не применяется).
- Просроченное исключение → `exception.expired`.

## Witness
- При `--write-witness` gate пишет JSON в:
  - `.agents/mcp/compas/witness/gate_ci-fast.json`
  - `.agents/mcp/compas/witness/gate_ci.json`
  - `.agents/mcp/compas/witness/gate_flagship.json`
- Gate output включает:
  - `witness_path`,
  - `witness { path, size_bytes, sha256, rotated_files }`.
- В receipts для каждого tool есть контрольные поля:
  - `stdout_bytes`, `stderr_bytes`,
  - `stdout_sha256`, `stderr_sha256`.
- Ротация witness enforced:
  - максимум 20 файлов,
  - максимум 2 MiB суммарно,
  - текущий файл gate никогда не удаляется.

## MCP server (stdio)
```bash
cargo run -p ai-dx-mcp
```
Tools: `compas.validate`, `compas.gate`, `compas.init`, `compas.catalog`, `compas.exec`.

`compas.catalog` отдаёт `plugin_id` для tools, чтобы агент видел владельца инструмента без чтения исходников.

## Build profiles (dist)
- **compas-full** (default): `cargo build -p ai-dx-mcp` (enables `full` → `external_packs`).
- **compas-lite**: `cargo build -p ai-dx-mcp --no-default-features` (fails closed on external packs).
- **WASM init-plugins (experimental)**: `cargo test -p ai-dx-mcp --no-default-features --features wasm` (deny-by-default imports; InitPlan-only output).

## Tool imports (без ручного дублирования в plugin.toml)
`plugin.toml` может импортировать любой набор `tool.toml` по glob:
```toml
[plugin]
id = "default"
tool_import_globs = ["tools/custom/**/tool.toml"]
```

Пример `tool.toml`:
```toml
[tool]
id = "cargo-test"
command = "cargo"
args = ["test"]
timeout_ms = 600000
max_stdout_bytes = 20000
max_stderr_bytes = 20000

[tool.env]
CARGO_TERM_COLOR = "always"
```

## Polyglot из коробки (без раздувания default plugin)
- Builtin packs уже включают: `rust`, `python`, `node-npm|node-pnpm|node-yarn`, `go`, `cmake`, `dotnet`.
- `compas.init` подключает только релевантные пакеты по детекторам lockfile/manifest, не раздувая текущий репо.
- Доказательство в тестах: `init_e2e_polyglot_validate_then_gate_ci_fast_dry_run_ok`
  (`crates/ai-dx-mcp/src/init/planner/tests.rs`) — gate wiring включает
  `npm-test`, `go-test`, `dotnet-test`, `cmake-*`, `python-test`, `rust-test`.
