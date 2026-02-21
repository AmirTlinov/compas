# P08 — Public Surface / Contract Break Guard (staged)

[LEGEND]
P08 = слот плагина для ужесточения `surface` + `contract_break`.
RATCHET_LOCK = текущий `quality_delta.config_changed` не разрешает менять active checks без baseline refresh.

[CONTENT]
P08 зафиксирован как отдельный plugin slot (`.agents/mcp/compas/plugins/p08/plugin.toml`) в staged-режиме:

- плагин присутствует в конфигурации и валиден;
- активные check-конфиги не изменяются (чтобы не ломать ratchet baseline);
- дальнейшее включение stricter `checks.surface`/`checks.contract_break` выполняется только вместе с официальным baseline refresh.

Итог: e2e validate/gate остаются зелёными, а точка интеграции P08 сохранена для следующего governance-окна.
