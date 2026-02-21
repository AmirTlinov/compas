# ARCHITECTURE

SSOT архитектуры проекта `compas`.
Источник данных для карты: `scripts/docs_sync.py`.

<!-- COMPAS_AUTO_ARCH:BEGIN -->
_fingerprint: f9173ff8da18f7b3_

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
| `p01` | Paranoid Tool Policy guardrail for strict tool execution | `p01-policy-guard` | `p01-policy-guard`, `cargo-test-lite` / `p01-policy-guard` / `p01-policy-guard` |

### Installed tools
| Tool | Owner plugin | Purpose | Command |
|---|---|---|---|
| `cargo-test` | `default` | Run cargo test (workspace) | `cargo` |
| `cargo-test-lite` | `default` | Cargo test (ai-dx-mcp, --no-default-features) | `cargo` |
| `cargo-test-wasm` | `default` | Cargo test (ai-dx-mcp, wasm feature on lite profile) | `cargo` |
| `docs-sync-check` | `default` | Verify that architecture docs and diagrams are in sync | `python3` |
| `p01-policy-guard` | `p01` | Validate plugin tool commands do not use shell binaries in strict mode | `python3` |

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
  P_default --> T_docs_sync_check["tool:docs-sync-check"]
  G --> T_docs_sync_check
  PL --> P_p01["plugin:p01"]
  P_p01 --> T_p01_policy_guard["tool:p01-policy-guard"]
  G --> T_p01_policy_guard
  TL --> T_cargo_test
  TL --> T_cargo_test_lite
  TL --> T_cargo_test_wasm
  TL --> T_docs_sync_check
  TL --> T_p01_policy_guard
```
<!-- COMPAS_AUTO_ARCH:END -->
