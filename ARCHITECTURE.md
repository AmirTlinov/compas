# ARCHITECTURE

SSOT архитектуры проекта `compas`.
Источник данных для карты: `scripts/docs_sync.py`.

<!-- COMPAS_AUTO_ARCH:BEGIN -->
_fingerprint: 1e8325672ac9ae85_

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
| `default` | MVP config for developing compas MCP in this repo | `cargo-test`, `cargo-test-lite`, `cargo-test-wasm`, `diff-scope-check`, `docs-sync-check`, `lint-unified`, `log-scan`, `p15-coverage`, `provenance-attestation`, `release-check`, `reuse-report`, `sbom`, `secrets-scan`, `semgrep`, `spec-check`, `tests-junit` | `docs-sync-check`, `cargo-test` / `docs-sync-check`, `cargo-test`, `cargo-test-lite` / `docs-sync-check`, `cargo-test`, `cargo-test-lite`, `cargo-test-wasm` |
| `p01` | Paranoid Tool Policy guardrail for strict tool execution | `p01-policy-guard` | `p01-policy-guard`, `cargo-test-lite` / `p01-policy-guard` / `p01-policy-guard` |
| `p02` | Spec/ADR gate plugin: enforce goal, non-goals, acceptance, edge-cases and rollback before implementation | — | `spec-check` / `spec-check` / `spec-check` |
| `p03` | P03 plugin enforces plan-to-diff scope consistency checks | — | `diff-scope-check` / `diff-scope-check` / `diff-scope-check` |
| `p04` | Architecture layers, contract boundaries, and boundary policy hardening | — | — / — / — |
| `p05` | P05 gate extension with reuse-report tool | — | `reuse-report` / `reuse-report` / `reuse-report` |
| `p06` | Complexity and LOC budgets for ai-dx-mcp changes | — | — / — / — |
| `p07` | Dead code and orphan API detection | — | — / — / — |
| `p08` | P08 staged integration: reserve plugin slot without changing active checks hash | — | — / — / `docs-sync-check` |
| `p09` | Supply-chain gate for deterministic dependency lockfiles and stable versions | — | — / — / — |
| `p10` | SBOM gate plugin for dependency manifests and lockfile traceability | — | `sbom` / `sbom` / `sbom` |
| `p11` | Provenance and attestation gate for release artifact integrity and trust. | — | `provenance-attestation` / `provenance-attestation` / `provenance-attestation` |
| `p12` | P12 wiring: add Semgrep security scan into gate flow | — | `semgrep` / `semgrep` / `semgrep` |
| `p13` | Secrets Leakage Guard plugin for blocking secret exposure checks. | — | `secrets-scan` / — / — |
| `p14` | P14 enforces normalized JUnit-aware test execution in gate | — | `tests-junit` / `tests-junit` / `tests-junit` |
| `p15` | Coverage non-regression guard for quality posture | — | `p15-coverage` / `p15-coverage` / `p15-coverage` |
| `p16` | P16 impact-to-gate wiring for runtime Rust changes | — | `cargo-test-wasm` / — / — |
| `p17` | Docs sync no-drift checks for architecture and documentation contract health | `p17-docs-no-drift` | `p17-docs-no-drift` / — / — |
| `p18` | Prevent PII and secret leaks in logging output | — | `log-scan` / `log-scan` / `log-scan` |
| `p19` | P19 plugin wires a unified lint gate for rust, python, and js/ts quality checks | — | `lint-unified` / `lint-unified` / `lint-unified` |
| `p20` | Performance Regression Budget gate for AI edits and runtime-impact checks. | `perf-bench` | `perf-bench` / `perf-bench` / `perf-bench` |
| `p21` | CI/CD release-readiness enforcement for deterministic publishing | — | `release-check` / `release-check` / `release-check` |

### Installed tools
| Tool | Owner plugin | Purpose | Command |
|---|---|---|---|
| `cargo-test` | `default` | Run cargo test (workspace) | `cargo` |
| `cargo-test-lite` | `default` | Cargo test (ai-dx-mcp, --no-default-features) | `cargo` |
| `cargo-test-wasm` | `default` | Cargo test (ai-dx-mcp, wasm feature on lite profile) | `cargo` |
| `diff-scope-check` | `default` | Check changed files against the explicit scope contract for plugin P03 | `python3` |
| `docs-sync-check` | `default` | Verify that architecture docs and diagrams are in sync | `python3` |
| `lint-unified` | `default` | Run unified lint checks (clippy first, then language linters when relevant) through one gate tool | `python3` |
| `log-scan` | `default` | Scan code and config files for potential PII or secret leakage through logging calls | `python3` |
| `p01-policy-guard` | `p01` | Validate plugin tool commands do not use shell binaries in strict mode | `python3` |
| `p15-coverage` | `default` | Coverage non-regression check against previous quality baseline | `python3` |
| `p17-docs-no-drift` | `p17` | Run docs sync no-drift validation across managed docs and supported generators | `python3` |
| `perf-bench` | `p20` | Compare performance baselines against current metrics and fail on budget regressions | `python3` |
| `provenance-attestation` | `default` | Run provenance/attestation evidence check for release artifact supply-chain integrity. | `python3` |
| `release-check` | `default` | Run release readiness checks for CI/CD stability and publish safety | `python3` |
| `reuse-report` | `default` | Emit a compact JSON report for P05 rollout/hardening checks | `python3` |
| `sbom` | `default` | Generate and validate lightweight SBOM metadata for dependency manifests | `python3` |
| `secrets-scan` | `default` | Run secrets leakage scan using semgrep, gitleaks, and trufflehog | `python3` |
| `semgrep` | `default` | Run semgrep SARIF scan for security baseline findings | `semgrep` |
| `spec-check` | `default` | Validate Spec/ADR gate artifacts (Goal, Non-goals, Acceptance, Edge-cases, Rollback) before code | `python3` |
| `tests-junit` | `default` | Run normalized JUnit-aware test checks for Rust and Python suites | `python3` |

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
  P_default --> T_log_scan["tool:log-scan"]
  G --> T_log_scan
  P_default --> T_p15_coverage["tool:p15-coverage"]
  G --> T_p15_coverage
  P_default --> T_provenance_attestation["tool:provenance-attestation"]
  G --> T_provenance_attestation
  P_default --> T_release_check["tool:release-check"]
  G --> T_release_check
  P_default --> T_reuse_report["tool:reuse-report"]
  G --> T_reuse_report
  P_default --> T_sbom["tool:sbom"]
  G --> T_sbom
  P_default --> T_secrets_scan["tool:secrets-scan"]
  G --> T_secrets_scan
  P_default --> T_semgrep["tool:semgrep"]
  G --> T_semgrep
  P_default --> T_spec_check["tool:spec-check"]
  G --> T_spec_check
  P_default --> T_tests_junit["tool:tests-junit"]
  G --> T_tests_junit
  PL --> P_p01["plugin:p01"]
  P_p01 --> T_p01_policy_guard["tool:p01-policy-guard"]
  G --> T_p01_policy_guard
  PL --> P_p02["plugin:p02"]
  PL --> P_p03["plugin:p03"]
  PL --> P_p04["plugin:p04"]
  PL --> P_p05["plugin:p05"]
  PL --> P_p06["plugin:p06"]
  PL --> P_p07["plugin:p07"]
  PL --> P_p08["plugin:p08"]
  PL --> P_p09["plugin:p09"]
  PL --> P_p10["plugin:p10"]
  PL --> P_p11["plugin:p11"]
  PL --> P_p12["plugin:p12"]
  PL --> P_p13["plugin:p13"]
  PL --> P_p14["plugin:p14"]
  PL --> P_p15["plugin:p15"]
  PL --> P_p16["plugin:p16"]
  PL --> P_p17["plugin:p17"]
  P_p17 --> T_p17_docs_no_drift["tool:p17-docs-no-drift"]
  G --> T_p17_docs_no_drift
  PL --> P_p18["plugin:p18"]
  PL --> P_p19["plugin:p19"]
  PL --> P_p20["plugin:p20"]
  P_p20 --> T_perf_bench["tool:perf-bench"]
  G --> T_perf_bench
  PL --> P_p21["plugin:p21"]
  TL --> T_cargo_test
  TL --> T_cargo_test_lite
  TL --> T_cargo_test_wasm
  TL --> T_diff_scope_check
  TL --> T_docs_sync_check
  TL --> T_lint_unified
  TL --> T_log_scan
  TL --> T_p01_policy_guard
  TL --> T_p15_coverage
  TL --> T_p17_docs_no_drift
  TL --> T_perf_bench
  TL --> T_provenance_attestation
  TL --> T_release_check
  TL --> T_reuse_report
  TL --> T_sbom
  TL --> T_secrets_scan
  TL --> T_semgrep
  TL --> T_spec_check
  TL --> T_tests_junit
```
<!-- COMPAS_AUTO_ARCH:END -->
