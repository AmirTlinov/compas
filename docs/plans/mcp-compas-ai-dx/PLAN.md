# PLAN — mcp-compas-ai-dx

## Цель
Собрать **MCP‑компас** (`compas`) — единую точку входа для агентов в инструменты проекта и в quality‑контур:
- `validate` (ratchet/strict/warn),
- `gate` (прогон проектных гейтов),
- `tools.*` (registry → describe → run) с детерминированными receipts и лимитами.

## Контекст и ограничения
- Цель — **контур**, а не “правила в Markdown”: MUST‑инварианты должны быть enforced инструментом.
- Минимум MCP‑методов (не плодим 999 инструментов).
- Fail‑closed по критичным инвариантам; fail‑open допустим только с **явной диагностикой**.
- Расширяемость через repo‑side config/plugins в `.agents/mcp/compas/` без костылей.

## Слайды (Slice-1..N)
1. **Slice‑1 (MVP skeleton)**: Rust+`rmcp` MCP‑сервер, config‑registry project‑tools, runner (timeout+tail), `validate` с 1 ratchet‑чеком (LOC), базовая структура планов/плагинов. **PASS (2026-02-14)** → см. `Slice-1.md`.
2. **Slice‑2**: LOC ratchet (practical) + exception protocol (allowlist с expiry) + gate witness (witness JSON) + CLI gate. **PASS (2026-02-14)** → см. `Slice-2.md`.
3. **Slice‑3**: env‑registry + `effective config` (единый реестр env vars и вывод конфигурации). **PASS (2026-02-14)** → см. `Slice-3.md`.
4. **Slice‑4**: boundary/public surface гейты (import/export rules + pub surface diff). **PASS (2026-02-14)** → см. `Slice-4.md`.
5. **Slice‑5**: receipts+witness (артефакты, sha256, ротация) + интеграция в `./dx ci-fast/flagship`. **PASS (2026-02-14)** → см. `Slice-5.md`.
6. **Slice‑6**: импорт project tools из `tool.toml` через `tool_import_globs` (plugin.toml) + fail‑closed ошибки `config.import_*`. **PASS (2026-02-14)** → см. `Slice-6.md`.
7. **Slice‑7**: rename бренда инструмента в `compas` + миграция конфиг-путей на `.agents/mcp/compas`. **PASS (2026-02-14)** → см. `Slice-7.md`.

## DoD (Definition of Done) плана
- MCP сервер реально запускается и возвращает корректные ответы на `validate`/`tools.list`/`tools.run`.
- Есть минимум 1 enforced ratchet‑проверка (LOC) + понятный отчёт.
- `gate` умеет запускать хотя бы один project tool и возвращать receipt (bounded output + duration).
- Конфигурация repo‑side описана и проверена (ошибки “кричат”, нет silent‑fail).

## Риски + Rollback
- Риск: “policy theater” (декларируем, но не enforce). Контрмера: тесты+ratchet в Slice‑1.
- Риск: разрастание API. Контрмера: удерживаем MCP surface ≤ 5 tools; расширения — через config/plugins.
- Rollback: удалить `crates/`, `Cargo.*`, `.agents/mcp/compas/`, `docs/plans/mcp-compas-ai-dx/`.

## BranchMind
- Workspace: 1
- Task: TASK-013
- Step: s:0 (Slice‑7)
