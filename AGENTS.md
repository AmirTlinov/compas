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
_fingerprint: 1efef80e6fdbcaba_

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
| `default` | MVP config for developing compas MCP in this repo | `cargo-test`, `cargo-test-lite`, `cargo-test-wasm`, `diff-scope-check`, `docs-sync-check`, `lint-unified`, `semgrep`, `spec-check` | `docs-sync-check`, `cargo-test` / `docs-sync-check`, `cargo-test`, `cargo-test-lite` / `docs-sync-check`, `cargo-test`, `cargo-test-lite`, `cargo-test-wasm` |
| `p01` | Paranoid Tool Policy guardrail for strict tool execution | `p01-policy-guard` | `p01-policy-guard`, `cargo-test-lite` / `p01-policy-guard` / `p01-policy-guard` |
| `p02` | Spec/ADR gate plugin: enforce goal, non-goals, acceptance, edge-cases and rollback before implementation | — | `spec-check` / `spec-check` / `spec-check` |
| `p03` | P03 plugin enforces plan-to-diff scope consistency checks | — | `diff-scope-check` / `diff-scope-check` / `diff-scope-check` |
| `p06` | Complexity and LOC budgets for ai-dx-mcp changes | — | — / — / — |
| `p08` | P08 staged integration: reserve plugin slot without changing active checks hash | — | — / — / `docs-sync-check` |
| `p09` | Supply-chain gate for deterministic dependency lockfiles and stable versions | — | — / — / — |
| `p12` | P12 wiring: add Semgrep security scan into gate flow | — | `semgrep` / `semgrep` / `semgrep` |
| `p16` | P16 impact-to-gate wiring for runtime Rust changes | — | `cargo-test-wasm` / — / — |
| `p19` | P19 plugin wires a unified lint gate for rust, python, and js/ts quality checks | — | `lint-unified` / `lint-unified` / `lint-unified` |

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
  P_default --> T_lint_unified["tool:lint-unified"]
  G --> T_lint_unified
  P_default --> T_semgrep["tool:semgrep"]
  G --> T_semgrep
  P_default --> T_spec_check["tool:spec-check"]
  G --> T_spec_check
  PL --> P_p01["plugin:p01"]
  P_p01 --> T_p01_policy_guard["tool:p01-policy-guard"]
  G --> T_p01_policy_guard
  PL --> P_p02["plugin:p02"]
  PL --> P_p03["plugin:p03"]
  PL --> P_p06["plugin:p06"]
  PL --> P_p08["plugin:p08"]
  PL --> P_p09["plugin:p09"]
  PL --> P_p12["plugin:p12"]
  PL --> P_p16["plugin:p16"]
  PL --> P_p19["plugin:p19"]
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
