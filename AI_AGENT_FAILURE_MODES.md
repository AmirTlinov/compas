# Каталог проблем, которые ИИ‑агенты часто привносят в проекты

[LEGEND]
FAILURE_MODE = Типовой класс ошибки (поведение агента, архитектурный анти‑паттерн или дефект качества), который повторяется в проектах и ухудшает скорость/надёжность/поддерживаемость.
SCOPE = Граница изменений в задаче; всё вне SCOPE должно считаться риском.
POLICY_THEATER = Политики/лимиты задекларированы, но не enforced (нет гейтов/проверок/рантайм‑контроля).
UNPLUGGED_IRON = Код/фича “есть”, но не подключена к реальным путям исполнения и не проверяется тестами/инвариантами.
PUBLIC_SURFACE = Публичная поверхность API (экспортируемые модули/типы/функции), которую сложно стабилизировать и легко разнести по всему дереву зависимостей.
STRINGLY_TYPED = Параметры/режимы представлены строками вместо типизированных enum/структур → хрупкость и некорректные состояния.
FAIL_OPEN = Ошибка/битые данные не останавливают поток (или маскируются) → “тихий провал” и сложная диагностика.
OBSERVABILITY = Нормальная наблюдаемость: структурированные логи/трейсы/метрики, а не ad‑hoc артефакты.
ENV_SPRAWL = Разрастание env‑флагов и конфигурации без “effective config” и центрального реестра.
GOD_MODULE = “Божественный” файл/модуль, в котором живёт почти всё (dispatch/оркестрация/политики/утилиты) → высокая когнитивная цена.

[CONTENT]

## 0) Примечание о “полноте”

- Этот каталог **не может быть полным навсегда**: в каждом проекте есть свои специфичные [FAILURE_MODE].
- Цель каталога: покрыть **основные оси риска** (архитектура, надёжность, тесты, безопасность, ops/DX) и дать язык для ревью/гейтов.
- Правило расширения: если вы поймали новый повторяющийся класс дефектов — добавьте его сюда и (по возможности) свяжите с enforce‑гейтом/чек‑листом.

## 1) Типовые ошибки поведения агента (как он работает)

1. Не делает discovery по репозиторию (поиск существующих реализаций/паттернов) → реализует уже имеющееся заново.
2. Берёт слишком широкий [SCOPE]: “поправлю рядом” (рефактор/переформат/ренейм/переписывание) вместо точечного изменения.
3. Преждевременные абстракции “на будущее” без 2+ реальных потребителей → лишние слои и связность.
4. Игнорирует принятый стиль/паттерны/нейминг/структуру модулей → смесь архитектур и ухудшение DX.
5. Поверхностно понимает домен → нарушает инварианты/контракты (API, схемы, миграции, форматы).
6. Путает симптом и причину: “чинит тест/линт” вместо первопричины; или меняет поведение без регрессионных тестов.
7. Пропускает edge cases: ошибки/таймауты/ретраи, конкурентность, производительность, локали/пути/кодировки.
8. Делает [FAIL_OPEN] там, где нужен fail‑closed: проглатывание ошибок/битых данных/`unwrap_or(default)` без диагностики.
9. Не проводит фичу через все точки входа (роуты/DI/экспорты/флаги/регистраторы) → [UNPLUGGED_IRON].
10. Не проверяет “живой” путь исполнения (e2e/smoke/golden), полагаясь на компиляцию/юнит‑тесты.
11. Делает шумные диффы (автоформат/регены/renames) поверх функционального изменения → дорогой ревью, сложный blame.
12. Платформенная слепота: пути/права/шелл/Windows/контейнеры/CI → “локально работает, в CI падает”.
13. Безопасность/приватность: утечки секретов/PII в логах/артефактах/тестах; опасные дефолты доступа.
14. Недетерминизм (время/рандом/порядок структур) без стабилизации/seed → flaky тесты и дрейф артефактов.
15. Не помечает допущения (assumptions) и не перепроверяет версии/даты/интерфейсы → “тихий дрейф” требований.
16. Не просит уточнений при неоднозначных требованиях и “додумывает” продуктовые решения → неожиданный UX/поведение.
17. Делает изменения без rollback/kill‑switch при риске (особенно в runtime/данных/контрактах) → откаты становятся опасны.
18. Не обновляет сопутствующую документацию/миграции/пример‑конфиги после изменения поведения → doc/config drift.
19. Делает “локально правильно”, но не доводит до production‑готовности: нет миграций, нет мониторинга, нет лимитов, нет error taxonomy.
20. Неправильный приоритет: оптимизирует микроскопическое (микро‑перф) вместо устранения крупной архитектурной причины деградаций.

## 2) Архитектурные и качественные анти‑паттерны (что агенты часто создают/усугубляют)

1. Лимиты LOC/FUNC/CC задекларированы, но не enforced гейтами → [POLICY_THEATER].
2. Массовые монолитные файлы (много >300 LOC, есть >1000 LOC) → высокая когнитивная цена правок/ревью.
3. Утечка границы core↔adapters через glob‑reexport (adapters видит “всё ядро”) → скрытая связность.
4. Слишком широкий [PUBLIC_SURFACE] ядра (много `pub mod`) → сложно стабилизировать API, легко разнести зависимости.
5. Staged/“ещё не подключённый” код под blanket `allow(dead_code)` → [UNPLUGGED_IRON].
6. [STRINGLY_TYPED] параметры на границах → хрупкость при расширениях, больше невалидных состояний.
7. [FAIL_OPEN] превращается в “тихий провал” (проглатывание ошибок/битых данных без диагностики).
8. Артефакты/леджеры растут без ротации/лимитов + O(n) “прочитать весь файл” → деградации по времени/диску.
9. [ENV_SPRAWL] (комбинаторика режимов) → хуже воспроизводимость; нужен “effective config”.
10. unsafe мутация env в тестах (процесс‑глобально) → флаки и неочевидные взаимодействия тестов.
11. Захардкоженные каталоги моделей/провайдеров + тесты на строки → дрейф и постоянный maintenance.
12. Тяжёлый dependency‑футпринт в core без feature‑флагов → сборка/бинарь/риски растут, даже когда подсистема не нужна.
13. Слабая [OBSERVABILITY] (нет стандартного tracing/log слоя; упор на ad‑hoc артефакты) → сложнее эксплуатация.
14. Дрейф/неравномерность док‑карты: high‑level доки слишком тонкие относительно сложности; MVP/пост‑MVP не синхронизированы.
15. Преобладание `anyhow` без доменных ошибок/типизированных инвариантов → хуже машинная обработка ошибок.
16. “Магические” эвристические константы (таймауты/лимиты/бюджеты) без централизации и объяснимого управления.
17. Внешние вызовы через `Command::new(...)` без capability‑detection слоя → неожиданные деградации на разных окружениях.
18. Циклические зависимости модулей/фич → сложно тестировать/удалять подсистемы, растёт время сборки.
19. Глобальное состояние (singleton/OnceCell/ленивые init) → порядок‑зависимость, скрытая связность, флаки.
20. Блокирующий I/O/CPU в async (или наоборот) → хвостовые задержки, подвисания, дедлоки.
21. Нет backpressure/лимитов параллелизма/отмены → OOM и деградации под нагрузкой.
22. Нет стратегии совместимости persisted state (БД/кэши/форматы) и downgrade → откаты опасны.
23. Доменные типы напрямую в wire/DTO → ломаются миграции и обратная совместимость.
24. Кодоген без pinned‑версий/детерминизма → постоянный дрейф и noisy diffs.
25. Тесты только unit без e2e/golden на ключевые user‑flows → “собирается, но не работает”.
26. Ошибки без стабильных error‑codes/категорий → нельзя корректно ретраить/обрабатывать/метрить.
27. `unwrap/expect/panic` в прод‑пути → крэш вместо управляемого отказа.
28. Платформенная хрупкость (пути/кодировки/права/shell) → баги на CI/Windows/контейнерах.
29. Перф‑анти‑паттерны в данных: N+1, отсутствие индексов, неоптимальные запросы/сканы, лишняя сериализация/копирование.
30. Утечки ресурсов: незакрытые файлы/дескрипторы/процессы, утечки памяти/буферов, накопление временных файлов.
31. Кэш без инвалидации/версионирования/границ размера → несогласованность данных и деградации.
32. Ретраи/таймауты без дисциплины: без backoff/джиттера/идемпотентности, либо “магические” значения размазаны по коду.
33. Смешивание слоёв (UI/инфра/домен) и нарушение “functional core / imperative shell” → рост связности и сложности удаления модулей.
34. Feature‑flag debt: флаги без владельца/срока, вечные эксперименты, противоречивые пары enable/disable → сложная матрица режимов.
35. Отсутствие строгой схемы конфигурации (валидатора/версионирования) и команды “показать effective config” → плохая воспроизводимость.
36. Supply‑chain слабости: зависимости не закреплены (версии/lockfile), нет сканирования уязвимостей/источников → риски безопасности и дрейф.
37. Лицензионный/комплаенс‑долг: нет SBOM/атрибуций/проверки лицензий зависимостей → юридические риски.
38. Нет стратегии совместимости публичных контрактов: deprecations, changelog, migration notes, semantic versioning → пользователи ломаются без предупреждения.
39. Неполные security‑гейты: нет статанализа/secret‑scanning/запрета опасных паттернов (path traversal, deserialization, shell‑exec) → баги ловятся поздно.
40. Нет “безопасной деградации” для внешних интеграций: circuit breaker, rate limits, идемпотентность, очереди → каскадные фейлы.
41. Неправильная работа с временем: wall‑clock вместо monotonic, timezone/DST, парсинг дат без тестов → трудноуловимые баги.
42. Неправильная работа с форматами/кодировками: UTF‑8/CRLF/locale, большие файлы, бинарные данные → ошибки на “живых” данных.
43. Отсутствие строгих границ на ввод/вывод: нет лимитов, квот, нормализации, защиты от больших payloads → DoS/деградации.
44. Отсутствие режима “dry‑run/validate‑only” для опасных операций (миграции, чистки, массовые правки) → повышенный риск инцидентов.
45. Тестовая стратегия не отражает риски: нет тестов на обратную совместимость, на деградацию перф‑характеристик, на corrupt‑inputs → сюрпризы в проде.

## 3) Пример “реально обнаруженных” проблем (аудит/инвентаризация)

> Ниже — предметный список, который был сформулирован в ходе обсуждения (с уровнями серьёзности).

### CRITICAL

1. 18 копий EnvGuard/EnvPatch/EnvScope/EnvRestore/EnvOverrideGuard — RAII‑guard для env‑переменных дублирован в 18 файлах под 5 именами.
2. 158 env‑переменных INTENT_* без центрального реестра и документации — разбросаны по 91 файлу, нет ENV_VARS.md, нет `--help`.
3. Plugin limits не enforced в рантайме — PluginLimits.llm_timeout_ms, max_artifact_bytes, max_payload_bytes, max_tool_stdout_bytes, tool_timeout_ms парсятся из манифеста, но никогда не проверяются (plugins/manifest.rs:105–117).
4. Permissions.secrets — мёртвая политика — поле объявлено в exec_policy/mod.rs:29, но никогда не вычисляется и не проверяется.

### HIGH

5. 5 копий sha256_hex — центральная crypto_util::sha256_hex существует, но pipeline/change_code.rs, models/manager.rs, fs_tools/write/apply.rs, tests/wasm_plugin_fixture_e2e.rs реализуют заново.
6. 2 копии compute_sha256 — идентичный код в knowledge_cards/authoring.rs:505 и knowledge_cards/runtime.rs:1177 (один модуль!).
7. 3 копии ScratchWorkspace — memory_smoke_eval/support.rs, chaos_dialog_eval/support.rs, memory_calibration_eval.rs — отличаются только строковым префиксом.
8. 19 идентичных event_* методов в turn_artifacts.rs:229–569 — отличаются одним строковым литералом, 11 из них мертвы.
9. 10 идентичных write_*_json методов в turn_artifacts.rs:612–696 — отличаются именем файла.
10. 3 копии path_allowed_by_scope() — fabricator/mod.rs, fabricator/fallback.rs, attempt.rs.
11. 2 копии normalize_scope_prefixes() — fabricator/mod.rs, attempt.rs.
12. 2 копии normalize_lines() — orchestrator_task.rs (functional), execution_pack.rs (imperative) — одна логика, разный стиль.
13. ~90 публичных модулей без тестов — включая sandbox/mod.rs, redaction.rs, cancel.rs, shell.rs, subprocess.rs, plugins/verify.rs, knowledge_cards/vector.rs, context_manager/trim.rs, fs_patch_dsl.rs.
14. Весь intent-adapters без тестов (кроме eval/ и нескольких TUI‑виджетов).
15. Функция cli.rs:run() — 1,792 строки — весь CLI dispatch в одном match.
16. unsafe { std::mem::transmute } для продления lifetime в knowledge_cards/vector.rs:502 — зависит от implementation detail крейта hnsw_rs.
17. 3 разные стратегии обработки ошибок для чтения JSON с диска — load_auth_store() → Result, load_time_lexicon() → Option, load_job_delivery_store() → bare default.
18. 3 стратегии обработки poisoned mutex в одном файле session/manager.rs — .unwrap(), .unwrap_or_else(|e| e.into_inner()), .map_err(|_| anyhow!(...)).
19. 67 публичных функций (9.4%) без внешних вызовов — API surface bloat.
20. 68 write‑only struct полей — записываются, никогда не читаются.
21. 6 orphaned функций BranchMind anchor linking API — ensure_anchor, upsert_anchor_link, remove_anchor_links_for_card, sync_card_anchors, list_anchor_links, all_cards — фича написана, но не подключена.

### MEDIUM

22. 4 глагола для чтения данных без семантического различия — get_, load_, fetch_, resolve_ для одинаковых операций.
23. 3 глагола для записи данных — save_, persist_, store_ (модуль auth использует все три).
24. Конструкторы: new vs from_env vs from_workspace vs from_config — SessionManager имеет и from_env, и new; Workspace имеет и new, и from_env_or_default.
25. Blanket #[allow(dead_code)] на весь модуль pipeline/change_code.rs:1 — скрывает любой мёртвый код.
26. Blanket #[allow(dead_code)] на весь impl TurnArtifacts (30+ методов) — turn_artifacts.rs:152.
27. Стейл #[allow(dead_code)] на активно вызываемых scroll_up/scroll_down — app.rs:203–211.
28. ChatUsage struct с 3 мёртвыми полями — llm.rs:1313–1335 — десериализуется и отбрасывается.
29. finish_reason — мёртвое поле — llm/stream.rs:16–17.
30. jsonrpc — мёртвое поле в RpcResponse — plugins/runtime.rs:310.
31. jsonrpc — мёртвое поле в RpcRequest — sessiond.rs:111–112.
32. service и replace — мёртвые поля в ServiceOverride — tests/spec_gate.rs:577–581.
33. Trait Sensor с 0 реализациями — diagnostics/sensors/mod.rs:10–13.
34. Enum ContextFormat с 1 вариантом AttentionOnly — context_manager/config.rs:5–9.
35. Trait MemorySource с 1 реализацией — knowledge_cards/sources.rs:30.
36. Trait ConsoleSink с 1 реализацией — tui/console/mod.rs:52.
37. KnowledgeConfig — 38 полей в плоской структуре — knowledge_cards/runtime.rs:31–82 — нужна декомпозиция на HnswConfig, VssConfig, RerankConfig, BranchMindConfig.
38. 40 env vars читаются в одном методе from_env() — knowledge_cards/runtime.rs.
39. 10 env vars в одном подмодуле auto_recall — без документации.
40. Позитивный + негативный флаг для одного концепта — INTENT_PIPELINE_SCOUTS_ENABLED и INTENT_RESEARCH_DISABLE_SCOUTS.
41. Теневые алиасы env vars — INTENT_TUI_DEBUG_MENU / INTENT_UI_DEBUG_MENU, INTENT_TUI_ADVANCED / INTENT_UI_ADVANCED.
42. INTENT_LLM_TIMEOUT_SECS читается в 3 файлах с разной fallback‑логикой — llm.rs, session_client.rs, sessiond.rs.
43. Дублирование env_u64/env_usize хелперов — wasm_client.rs:452–464 не знает о env_util.rs.
44. 2 копии env_lock() с OnceLock<Mutex<()>> — fs_tools/write/apply.rs:1191, pipeline/change_code.rs:378 — не используют test_support::ENV_LOCK.
45. 5 копий EnvPatch в тестах — не используют (ещё не существующий) общий test_support::EnvPatch.
46. 3 копии anchor() хелпера — execution_pack/tests.rs, fabricator/tests.rs, change_code.rs.
47. 3 копии идентичного конструктора ContextConfig в одном файле — agent/memory_write/tests.rs:62,158,241.
48. Циркулярная зависимость context_manager ↔ episodic — production‑модули взаимно импортируют друг друга.
49. turn_coordinator_change_code — distributed god module — 6 файлов, каждый с 9–16 импортами crate‑модулей.
50. Mutex .lock().unwrap() в production — models/manager.rs:291, knowledge_cards/vss.rs:63,79,121,160,192 — panic при poison.
51. Нет структурированного логирования — ни tracing, ни log — только eprintln!/println!.
52. eprintln! и println! перемешаны в одних eval‑блоках cli.rs.
53. EvidencePruneResult — 4 write‑only метрики — removed_count, scanned, skipped_protected, skipped_recent.
54. EvidenceReceipt.input_excerpt, output_excerpt — audit trail записывается, но не отображается.
55. PlanSliceV1 — 4 write‑only поля — dependencies, dod, implementation_anchors, non_goals.
56. Тесты agent/tests.rs:64–83 привязаны к точным строкам system prompt — ломаются при переформулировании.
57. Тест canonical_kind_order_matches_plan_defaults привязан к hardcoded списку — execution_pack/tests.rs:162–170.
58. diagnostics/baseline.rs — 2 теста, нет тестов на corrupt file, write failure, negative values.
59. diagnostics/report.rs — 1 тест на весь HealthReport модуль.
60. i18n/lang_detect.rs — нет тестов для пустых строк, emoji, whitespace‑only.
61. pipeline/policy.rs — 1 happy‑path тест, нет edge cases.

### LOW

62. Смешение mod.rs и файлового стиля модулей в одном дереве — pipeline/mod.rs + pipeline/turn_coordinator.rs + turn_coordinator/.
63. Избыточное имя директории turn_coordinator_change_code — уже внутри turn_coordinator/, должно быть change_code/.
64. 5 уровней вложенности — pipeline/turn_coordinator/turn_coordinator_change_code/stages/verifier.rs.
65. anchor_tags.rs — все 5 функций pub, но используются только внутри intent-core — должны быть pub(crate).
66. LlmMessage::system/user/assistant принимают String — llm.rs:123,130,137 — должны быть impl Into<String>.
67. normalize_llm_base_url принимает String, хотя работает с &str — llm.rs:1179.
68. 23 экземпляра .collect::<Vec<_>>().join() — лишняя аллокация.
69. url.clone() вместо url.as_str() при вызове reqwest::get() — llm.rs:608,778,828,915.
70. .to_string().as_str() anti‑pattern — auto_recall.rs:1386.
71. Стейл doc comment "In V1" — diagnostics/baseline.rs:79–81 — код обрабатывает V1 и V2 одинаково.
72. #[allow(unused_imports)] на реально неиспользуемом re-export — fs_tools/write/mod.rs:23–24.
73. 4 тривиальных validate() обёртки над validate_invariants() — work_contracts.rs:248,446,617,707 — 3 из 4 с #[allow(dead_code)].
74. Redundant empty trait method overrides в ChannelSink — apex_glass/agent_worker.rs:94–96.
75. ort = "2.0.0-rc.11" — зависимость от release candidate — API может измениться до 2.0.0.
76. Magic number 1200 (max description length) без named constant — pipeline/orchestrator_task.rs:130.
77. Magic number 240 (max guidance length) без named constant — pipeline/execution_pack/tests.rs:244.
78. Magic number 2000 (line scan limit) в production коде — diagnostics/signatures.rs:56.
79. ContextConfig — 15 полей, borderline — context_manager/config.rs:33–55 — attention_router_* поля можно группировать.
80. FsSearchConfig — 16 полей — fs_search/mod.rs:24–44 — rerank_* поля можно группировать.
81. has_any_token() -> bool глотает ошибки, тогда как load_token() -> Result<Option<String>> их пробрасывает — auth/credentials.rs:99,132 — одна операция, две стратегии.
82. fetch_transcript_window_best_effort() -> Option<Value> vs fetch_episode_summaries() -> Vec<String> — обе fetch из internal store, разные return types.
83. dialoguer и rustyline — оба присутствуют — оба устаревшие при fullscreen TUI.
84. 41 single‑use private функций — кандидаты на инлайнинг (часть оправдана для читаемости).
85. wasmtime и ort не за feature flag — тяжёлые зависимости, но compile‑time cost несут все сборки.
