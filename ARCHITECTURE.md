# ARCHITECTURE

SSOT архитектуры проекта `compas`.
Источник данных для карты: `scripts/docs_sync.py`.

<!-- COMPAS_AUTO_ARCH:BEGIN -->
_fingerprint: ee31cecc77836f1a_

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
| `p17` | Docs sync no-drift checks for architecture and documentation contract health | `p17-docs-no-drift` | `p17-docs-no-drift` / — / — |

### Installed tools
| Tool | Owner plugin | Purpose | Command |
|---|---|---|---|
| `cargo-test` | `default` | Run cargo test (workspace) | `cargo` |
| `cargo-test-lite` | `default` | Cargo test (ai-dx-mcp, --no-default-features) | `cargo` |
| `cargo-test-wasm` | `default` | Cargo test (ai-dx-mcp, wasm feature on lite profile) | `cargo` |
| `diff-scope-check` | `default` | Check changed files against the explicit scope contract for plugin P03 | `python3` |
| `docs-sync-check` | `default` | Verify that architecture docs and diagrams are in sync | `python3` |
| `p17-docs-no-drift` | `p17` | Run docs sync no-drift validation across managed docs and supported generators | `python3` |

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
  PL --> P_p17["plugin:p17"]
  P_p17 --> T_p17_docs_no_drift["tool:p17-docs-no-drift"]
  G --> T_p17_docs_no_drift
  TL --> T_cargo_test
  TL --> T_cargo_test_lite
  TL --> T_cargo_test_wasm
  TL --> T_diff_scope_check
  TL --> T_docs_sync_check
  TL --> T_p17_docs_no_drift
```
<!-- COMPAS_AUTO_ARCH:END -->
