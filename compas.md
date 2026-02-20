# AI_AGENT_FAILURE_MODES

Этот документ фиксирует **режимы отказа (failure modes)**, характерные для разработки ПО, когда код пишут **только ИИ‑агенты**, а человек **не читает/не правит код руками**.

Контекст (на базе анализа Intent):
- Репо живёт на локальных гейтах (`./dx ci-fast/ci/flagship`) и детерминированных witness‑артефактах.
- Цель: *AI‑native DX*, где нормы — это **исполняемые контракты**, а не “красивые правила в Markdown”.
- Риск‑профиль: высокий дрейф (агенты оптимизируют “сейчас работает”), поэтому нужен **ratchet** (не ухудшать) + **fail‑closed** на критичных инвариантах.

---

## 0) Термины

- **Policy theater** — правило объявлено, но не enforced машиной.
- **Unplugged iron** — код/контракт “есть”, но нет реального пути исполнения, доказательств, и/или теста, который гарантирует достижимость.
- **Ratchet** — гейт, запрещающий *ухудшать* метрику (даже если текущий уровень уже плохой).
- **Fail‑open** — пользовательский UX не должен падать. Важно: fail‑open ≠ silent‑fail.
- **Silent‑fail** — ошибка/деградация проглочена без диагностического следа.
- **Exception protocol** — единственный легальный способ нарушить правило: allowlist + причина + срок + тест‑барьер от расползания.

---

## 1) Карта типовых failure modes (агентские)

Ниже — список “что ломается”, *как это заметить*, и *как лечить системно*.

### FM‑01. Policy theater (правила ≠ реальность)
**Симптомы:** в `REPO_RULES.md`/арх‑доках есть лимиты/нормы, но они не проверяются и регулярно нарушаются.
**Причина:** агент ориентируется на ближайший успех (запустить тесты), а не на дисциплину качества.
**Детектор:** любой “normative” документ без соответствующего `./dx validate`/теста.
**Лечение:**
- “Norms → Gates”: каждое MUST‑правило должно иметь check.
- Для больших долгов — ratchet (не ухудшать) вместо “сразу исправить всё”.

### FM‑02. Монолитизация файлов/функций (контекстный взрыв)
**Симптомы:** файлы по 1–3k LOC, функции по сотни строк.
**Причина:** агенты складывают изменения в “ближайшее место” и избегают болезненной декомпозиции.
**Детектор:** LOC/FUNC/CC метрики растут от коммита к коммиту.
**Лечение:**
- LOC/FUNC/CC ratchet в validate.
- Запрет новых файлов > лимита.
- Авто‑подсказки “куда распилить” (карта модулей) в отчёте валидатора.

### FM‑03. Размывание границ (boundary leaks)
**Симптомы:** glob‑reexport, “всё импортируем отовсюду”, core начинает знать про IO/UX.
**Причина:** агент минимизирует churn и ускоряет компиляцию “прямо сейчас”.
**Детектор:** запретные паттерны импортов/экспортов; дрейф public surface.
**Лечение:**
- Boundary‑gate (regex/AST‑checks) + список разрешённых “портов”.
- Public‑API diff gate: изменения `pub` требуют явного обоснования.

### FM‑04. Unplugged iron / staged‑код
**Симптомы:** `allow(dead_code)`, “инструмент/модуль готов, но wiring потом”.
**Причина:** агент “планирует” кодом, а не через план/ADR.
**Детектор:** staged‑модули без минимального e2e пути/теста/feature‑flag.
**Лечение:**
- Правило: staged‑код допускается только:
  1) за feature‑flag,
  2) с минимальным тестом достижимости,
  3) с expiry‑датой в exception.

### FM‑05. Stringly‑typed интерфейсы
**Симптомы:** режимы/команды/состояния на строках, слабая типизация границ.
**Причина:** быстрее “склеить”, чем вводить enum/value‑objects.
**Детектор:** много `String` там, где есть конечные множества (mode, state, action).
**Лечение:**
- Внешние границы: JSON‑schema + строгий парсинг в enum.
- Внутренние: семантические типы (newtype) и `#[non_exhaustive]` там, где нужен рост.

### FM‑06. Fail‑open → Silent‑fail (не наблюдаем деградации)
**Симптомы:** “оно иногда не работает”, но нет следов почему.
**Причина:** агент “не хочет падать” и начинает проглатывать ошибки.
**Детектор:** `Result` превращается в `Ok(())` без счётчика/ивента.
**Лечение:**
- Правило: **каждый fail‑open fallback обязан оставить сигнал** (счётчик/diagnostic event).
- Валидация: grep/AST‑checks на “проглатывание” без метрики.

### FM‑07. Артефакт‑спролл / долговременные файлы без ротации
**Симптомы:** JSONL/лог/ledgers растут бесконечно; появляются O(n) чтения всего файла.
**Причина:** агент не моделирует “месяцы эксплуатации”, только “сейчас тест прошёл”.
**Детектор:** чтение `read_to_string` больших файлов; отсутствие TTL/rotate.
**Лечение:**
- Артефакт‑политика: max size, rotate, tail‑read.
- Gate: запрет full‑scan в hot‑path.

### FM‑08. Env‑sprawl (конфиг‑комбинаторика)
**Симптомы:** десятки env‑флагов; воспроизводимость падает.
**Причина:** “добавим ручку” проще, чем спроектировать конфиг.
**Детектор:** новые `INTENT_*` без реестра/доков/effective config.
**Лечение:**
- Env‑registry gate: новый флаг → запись + тест + вывод “effective config”.

### FM‑09. Тяжёлые зависимости “всегда включены”
**Симптомы:** ORT/tokenizers/tantivy/wasmtime тащатся всем, даже когда не нужны.
**Причина:** агент оптимизирует функциональную полноту, а не матрицу сборок.
**Детектор:** зависимости без feature‑flag и без профиля сборки.
**Лечение:**
- Feature matrix + gate: минимальный профиль должен собираться быстро.

### FM‑10. Дрейф docs vs реальность
**Симптомы:** `GOALS/ARCHITECTURE` отстают; правила/карта вводят новых агентов в заблуждение.
**Причина:** агент закрывает “код работает”, а не “карта актуальна”.
**Детектор:** док‑гейт (doc graph, orphans, stale claims).
**Лечение:**
- Требовать “doc delta” для изменений архитектуры/гейтов.

### FM‑11. Ошибки как строки (anyhow‑везде)
**Симптомы:** нет доменных ошибок/кодов, трудно машинно обрабатывать и валидировать поведение.
**Причина:** агенты выбирают кратчайший путь.
**Детектор:** границы возвращают `anyhow::Error` без error kinds.
**Лечение:**
- Доменные ошибки на границах/протоколах; `thiserror` + error codes.

### FM‑12. Внешние инструменты без capability detection
**Симптомы:** `Command::new("gcloud")/"az"/"codex"` и т.п. ведут себя по‑разному на окружениях.
**Причина:** агент разрабатывает в одном окружении.
**Детектор:** нет “preflight” проверки, нет нормальных сообщений об отсутствии бинаря.
**Лечение:**
- preflight checks + явные “capability flags”.

---

## 2) Как лечить “на корню”: контур вместо промпта

### Минимальная формула
1) **Validator‑компас** (1 команда, 1 отчёт) — `compas validate`.
2) **Gate** — `compas gate` (fmt/clippy/test/flagship + witness).
3) **Ratchet** в validate — запрещает ухудшать метрики.
4) **Exception protocol** — единственный легальный обход.

Промпт агента должен ссылаться на инструмент (“перед финалом прогоняй gate”), но **не заменять** контур.

---

## 3) Предложение: Rust‑архитектура универсального MCP‑инструмента `compas`

Цель: единая точка входа для агента в любом репо.
- Core универсален.
- Репо‑специфика подключается как плагины/политики в `.agents/mcp/compas/plugins/*`.
- Инструмент может проксировать/запускать “любые инструменты проекта” через единый интерфейс.

### 3.1. Workspace (cargo workspace)

**Вариант (рекомендованный):**
- `crates/compas-core/` — доменная логика: registry, checks, policies, receipts, baseline/ratchet.
- `crates/compas-runner/` — запуск команд/скриптов: cwd/env/таймауты/stream capture.
- `crates/compas-plugins/` — загрузка репо‑плагинов (config‑plugins + опц. WASM‑plugins).
- `crates/compas-mcp/` — MCP сервер (stdio): экспонирует `validate/gate/plugins.*/tools.*`.
- `crates/compas-cli/` (опционально) — локальная CLI обёртка (удобно для людей).

### 3.2. Публичные MCP методы (минимальный API)

- `validate({mode: ratchet|strict|warn, repo_root, profile}) -> Report`
- `gate({kind: ci-fast|ci|flagship, repo_root}) -> GateResult + witness`
- `plugins.list({repo_root}) -> [PluginInfo]`
- `plugins.describe({plugin_id}) -> PluginSpec`
- `tools.list({repo_root}) -> [Tool]`
- `tools.describe({tool_id}) -> ToolSpec`
- `tools.run({tool_id, args, confirm, dry_run}) -> Receipt`
- `docs-sync` (wrapper) обновляет auto-managed архитектурные блоки в `ARCHITECTURE.md` и `AGENTS.md`.

**Receipt** должен быть детерминированным и коротким:
- exit_code
- bounded stdout/stderr (tail)
- duration
- produced_artifacts (paths/sha)
- violations (если validate встроен)

### 3.3. Plugin model (repo‑side)

Репо хранит плагины в:
- `.agents/mcp/compas/plugins/<plugin>/plugin.toml`
- `.agents/mcp/compas/plugins/<plugin>/checks/*.toml` (если нужно дробить)
- `.agents/mcp/compas/plugins/<plugin>/allowlist.toml` (исключения)

Содержимое `plugin.toml`:
- строгий стандарт:
  - `plugin.id`/`tool.id` по шаблону `^[a-z0-9][a-z0-9_-]{1,63}$`,
  - `plugin.description`/`tool.description` обязательны и краткие (12..220),
  - неизвестные поля в манифестах запрещены (fail-closed),
  - `tool.command` обязателен,
  - ссылки gate на неизвестные tools запрещены (fail-closed).
- `checks`:
  - loc ratchet (thresholds + allowlist)
  - boundary rules (deny patterns)
  - public api surface rules
  - env registry rules
  - artifact hygiene
  - doc graph hooks (какую команду вызвать)
- `tools`:
  - wrappers на `./dx`, `make`, `cargo`, `scripts/*`, `tools/custom/**/tool.toml` (импорт)

**Code plugins** (если понадобится):
- WASM‑плагины с ограничениями (эпохи/таймауты/лимиты памяти) — чтобы не тащить зависимости.

### 3.4. Baseline + Ratchet

- Baseline хранить в репо (или в `.intent/proofs/` если это “локальная метрика”).
- Ratchet правило: “не ухудшать” относительно baseline.
- Для строгих must‑инвариантов baseline не нужен: это hard‑fail.

### 3.5. Exception protocol

- Исключения живут только в одном месте (allowlist), имеют:
  - id
  - rule
  - path matcher
  - reason
  - owner (может быть “team/agent”)
  - expires_at
  - test_barrier (какой тест/гейт не даст расползтись)

---

## 4) Практический MVP‑план внедрения (без “второго монстра”)

1) Start config‑only: встроенные checks + plugin.toml.
2) Встроить `./dx validate` (или `compas validate`) в `ci-fast` и `flagship`.
3) Добавить receipts + witness.
4) Только после этого думать о code‑plugins.

---

## 5) Invariant Agent Compass (синтез 40+ BranchMind idea branches)

Чтобы compas оставался универсальным и lean (без “101 инструмент и 999 вызовов”), архитектура фиксируется как набор инвариантов:

1. **Contract‑first**: все выходы validate/gate строго типизированы и стабильны по кодам.
2. **Evidence‑first**: любое значимое действие обязано оставлять machine‑proof (`CMD/LINK/FILE/CODE_REF`).
3. **Enforcement‑first**: “декларация без проверки” считается багом (policy theater = violation).
4. **Minimal surface**: минимум MCP surface + композиция, а не рост количества инструментов.
5. **Plugin ergonomics**: расширение через простой repo‑side manifest и предсказуемый test harness.
6. **Fail‑closed matrix**: ошибки конфигурации/контрактов блокируют verify/gate.
7. **Coverage ratchet**: покрытие failure‑mode каталога измеряется и не должно деградировать.
8. **Trust transparency**: trust‑оценка выводима из findings и публичных весов, без black‑box.

Практическая фиксация инварианта в текущем репо:
- canonical failure-mode catalog вынесен в `.agents/mcp/compas/failure_modes.toml`,
- `validate.coverage` считает покрытие по этому каталогу,
- невалидный каталог даёт `failure_modes.invalid` (fail‑closed).
