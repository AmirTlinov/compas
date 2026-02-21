# AGENTS

## Rules (max 10 lines)
1) `cargo test -p ai-dx-mcp` обязателен перед любым финалом.
2) `cargo run -p ai-dx-mcp -- validate ratchet` обязателен после правок.
3) `./dx ci-fast --dry-run` обязателен как итоговый локальный гейт.
4) Любой новый инструмент добавляется только через `tool.toml`.
5) Любой новый плагин добавляется только через `plugin.toml`.
6) Описания plugin/tool обязательны и должны быть короткими и ясными.
7) Любая ошибка конфигурации считается блокирующей (fail-closed).
8) Архитектуру правим через `./dx docs-sync`, не вручную в auto-блоках.
9) Изменения делаем малыми и доводим до зелёного Verify.
10) Истина в коде и автосинке; compas держим lean: минимум инструментов/команд/семантики при максимальном покрытии задач AI‑разработки; расширяемость плагинами и подключение существующих инструментов должны быть простыми (не “полоса препятствий”, не 101 инструмент и 999 вызовов ради 1 действия).

## Architecture quick map
Ниже auto-managed карта, синхронизация: `./dx docs-sync`.

<!-- COMPAS_AUTO_ARCH:BEGIN -->
_fingerprint: 4a3a6b95990344eb_

## Runtime Map (auto)

### Core paths
| Segment | Path |
|---|---|
| MCP server | `crates/ai-dx-mcp/src/server.rs` |
| Runtime pipeline | `crates/ai-dx-mcp/src/{app,repo,runner}.rs` |
| Plugin configs | `.agents/mcp/compas/plugins/*/plugin.toml` |
| Tool manifests | `tools/custom/**/tool.toml` |
| Docs sync script | `scripts/docs_sync.py` |
| DX wrapper | `dx` |

### Installed plugins
| Plugin | Purpose | Tools | Gates (ci-fast / ci / flagship) |
|---|---|---|---|
| `default` | MVP config for developing compas MCP in this repo | `cargo-test`, `cargo-test-lite`, `cargo-test-wasm`, `docs-sync-check` | `docs-sync-check`, `cargo-test` / `docs-sync-check`, `cargo-test`, `cargo-test-lite` / `docs-sync-check`, `cargo-test`, `cargo-test-lite`, `cargo-test-wasm` |
| `p09` | Supply-chain gate for deterministic dependency lockfiles and stable versions | — | — / — / — |

### Installed tools
| Tool | Owner plugin | Purpose | Command |
|---|---|---|---|
| `cargo-test` | `default` | Run cargo test (workspace) | `cargo` |
| `cargo-test-lite` | `default` | Cargo test (ai-dx-mcp, --no-default-features) | `cargo` |
| `cargo-test-wasm` | `default` | Cargo test (ai-dx-mcp, wasm feature on lite profile) | `cargo` |
| `diff-scope-check` | `default` | Check changed files against the explicit scope contract for plugin P03 | `python3` |
| `docs-sync-check` | `default` | Verify that architecture docs and diagrams are in sync | `python3` |
| `lint-unified` | `default` | Run unified lint checks (clippy first, then language linters when relevant) through one gate tool | `python3` |
| `p01-policy-guard` | `p01` | Validate plugin tool commands do not use shell binaries in strict mode | `python3` |
| `semgrep` | `default` | Run semgrep SARIF scan for security baseline findings | `semgrep` |
| `spec-check` | `default` | Validate Spec/ADR gate artifacts (Goal, Non-goals, Acceptance, Edge-cases, Rollback) before code | `python3` |

### MCP surface
`compas.catalog`, `compas.exec`, `compas.gate`, `compas.init`, `compas.validate`

### Mermaid
```mermaid
flowchart LR
  Agent[AI Agent] --> DX[./dx]
  DX --> MCP[compas MCP]
  MCP --> V[validate]
  MCP --> G[gate]
  MCP --> PL[plugins.*]
  MCP --> TL[tools.*]
  V --> C1[loc/env/boundary/public-surface]
  PL --> P_default["plugin:default"]
  P_default --> T_cargo_test["tool:cargo-test"]
  G --> T_cargo_test
  P_default --> T_cargo_test_lite["tool:cargo-test-lite"]
  G --> T_cargo_test_lite
  P_default --> T_cargo_test_wasm["tool:cargo-test-wasm"]
  G --> T_cargo_test_wasm
  P_default --> T_diff_scope_check["tool:diff-scope-check"]
  G --> T_diff_scope_check
  P_default --> T_docs_sync_check["tool:docs-sync-check"]
  G --> T_docs_sync_check
  PL --> P_p09["plugin:p09"]
  TL --> T_cargo_test
  TL --> T_cargo_test_lite
  TL --> T_cargo_test_wasm
  TL --> T_diff_scope_check
  TL --> T_docs_sync_check
  TL --> T_lint_unified
  TL --> T_p01_policy_guard
  TL --> T_semgrep
  TL --> T_spec_check
```
<!-- COMPAS_AUTO_ARCH:END -->
