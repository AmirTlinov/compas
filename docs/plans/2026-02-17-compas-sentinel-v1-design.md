# Compas Sentinel v1: Design Document

**Status**: Approved
**Date**: 2026-02-17
**Scope**: Invariant quality gate for AI-agent code with anti-gaming protection

---

## 1. Goals

Make compas an invariant quality gate where:

- An agent cannot close a step without proof (verdict + witness chain).
- Quality cannot be gamed by optimizing for formal gate passage while degrading actual code.
- Extension happens through plugins/packs, not API surface growth.

### Chosen defaults

| Parameter | Value |
|---|---|
| Compatibility | Alpha-break (legacy/compat not priority) |
| Autonomy | Guarded medium (verdict, not orchestration) |
| Security depth | Max strict with plugin-first mature tooling |
| Role | Instrument + judge (verdict, not execution control) |

---

## 2. Architecture

### 2.1 Role: Instrument + Judge

compas issues a **verdict** (`pass` / `retryable` / `blocked`) but does **not** manage retry/rollback. The agent decides what to do with the verdict. The 5-tool MCP surface stays unchanged.

### 2.2 Two-Phase Validate Flow

```
Phase 1 (existing checks):
  repo_config -> boundary/loc/surface/duplicates/supply_chain/tool_budget/env_registry
    -> violations_raw (BEFORE allowlist)
    -> apply_allowlist -> violations_filtered + suppressed
    -> to_findings_v2(violations_raw) -> insights_raw (trust/coverage/risk from RAW)
    -> to_findings_v2(violations_filtered) -> insights_display (for user)
    = trust_score_raw, coverage_raw, risk_raw   <- STABLE, not recomputed

Phase 2 (quality_delta -- UNIFIED ratchet):
  quality_delta(insights_raw, snapshot_baseline)
    -> violations_phase2 (trust_regression, coverage_regression, risk_profile_regression,
                          loc_regression, surface_regression, duplicates_regression,
                          scope_narrowed, config_changed)

Phase 3 (judge -- SINGLE decision point):
  judge(violations_filtered + violations_phase2, receipts?, checks_presence)
    -> Verdict { decision, action_plan, policy_trace }

Phase 4 (assembly):
  violations_final = violations_filtered + violations_phase2
  findings_v2_final = to_findings_v2(violations_final)
  trust_score = trust_score_raw    <- NOT recomputed (breaks circular dependency)
  ok = blocking_violations_final.is_empty() || mode == Warn
  verdict = from Phase 3
```

### 2.3 Circular Dependency Resolution

quality_delta compares against the baseline using a **pre-delta trust score** computed from all violations *excluding* `quality_delta.*` violations. quality_delta violations appear in the final output but do NOT feed back into the trust score.

### 2.4 quality_delta Replaces Per-Check Ratchet

Existing ratchet branches in `loc.rs`, `surface.rs`, `duplicates.rs` are **removed**. quality_delta becomes the sole ratchet mechanism:

- `loc.rs` always runs in strict mode (checks `max_loc` threshold, no baseline comparison)
- `surface.rs` always checks `max_items` without baseline comparison
- `duplicates.rs` detects duplicates without ratchet
- All ratchet logic (baseline comparison, regression detection) lives in `quality_delta.rs` with a **unified snapshot**

Phase-1 loc/surface/duplicates violations get `ViolationTier::Observation` (tracked, not individually blocking). Only quality_delta violations are blocking.

### 2.5 Raw vs Display Metrics

quality_delta compares **raw** (pre-suppression) signals. This prevents allowlist gaming where an agent adds allowlist entries to improve the delta before writing a baseline.

API exposes both:
- `quality_posture` — raw trust/coverage/risk (for ratchet comparison)
- `trust_score` / `risk_summary` / `coverage` — display metrics (post-suppression, for user)

---

## 3. API Contract (Schema v3)

### 3.1 New Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViolationTier {
    Blocking,     // Blocks validate/gate
    Observation,  // Tracked for quality_delta ratchet, does not block alone
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualityPosture {
    pub trust_score: i32,
    pub trust_grade: String,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Verdict {
    pub decision: Decision,
    pub action_plan: ActionPlan,
    pub policy_trace: PolicyTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Decision {
    pub status: DecisionStatus,
    pub reasons: Vec<DecisionReason>,
    pub blocking_count: usize,
    pub observation_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Pass,
    Retryable,
    Blocked,
}
```

### 3.2 ValidateOutput Changes

```rust
pub struct ValidateOutput {
    pub ok: bool,                              // backward compat
    pub schema_version: String,                // "3"
    pub quality_posture: Option<QualityPosture>, // NEW: raw metrics
    pub verdict: Option<Verdict>,              // NEW: structured decision
    pub violations: Vec<Violation>,            // each now has tier field
    // ... all existing fields unchanged ...
}
```

### 3.3 GateOutput Changes

```rust
pub struct GateOutput {
    pub ok: bool,
    pub verdict: Option<Verdict>,              // NEW
    // ... all existing fields unchanged ...
}
```

### 3.4 ValidateRequest Changes

```rust
pub struct ValidateRequest {
    pub repo_root: Option<String>,
    pub mode: ValidateMode,
    pub write_baseline: Option<bool>,
    pub baseline_maintenance: Option<BaselineMaintenance>,  // NEW
}

pub struct BaselineMaintenance {
    pub reason: String,   // min 20 chars
    pub owner: String,    // who authorized
}
```

### 3.5 Backward Compatibility

- `ok` field preserved with original semantics (but now considers ViolationTier)
- `schema_version` bumped from "2" to "3"
- All new fields are `Option` — consumers ignoring them see no difference
- Contract tests: MCP smoke test and CLI smoke test verify v2 consumers work with v3 output

---

## 4. Judge Module

### 4.1 Table-Driven Classification

Single source of truth for violation code -> (ErrorClass, ViolationTier) mapping:

```rust
// src/judge/registry.rs

pub struct ViolationClassEntry {
    pub pattern: ViolationPattern,
    pub class: ErrorClass,
    pub tier: ViolationTier,
}

pub enum ViolationPattern {
    Exact(&'static str),
    Prefix(&'static str),
    Suffix(&'static str),
}

pub static VIOLATION_REGISTRY: &[ViolationClassEntry] = &[
    // Config / structural
    entry(Prefix("config."),                  SchemaConfig,      Blocking),
    entry(Prefix("failure_modes."),           SchemaConfig,      Blocking),
    entry(Prefix("pack."),                    SchemaConfig,      Blocking),
    entry(Exact("exception.allowlist_invalid"), SchemaConfig,    Blocking),

    // Infrastructure failures (check could not run)
    entry(Suffix(".check_failed"),            RuntimeRisk,       Blocking),
    entry(Suffix(".read_failed"),             RuntimeRisk,       Blocking),
    entry(Suffix(".stat_failed"),             RuntimeRisk,       Blocking),
    entry(Suffix(".manifest_parse_failed"),   RuntimeRisk,       Blocking),

    // Security
    entry(Prefix("supply_chain."),            Security,          Blocking),
    entry(Exact("security.allow_any_policy"), Security,          Blocking),

    // Quality regression
    entry(Prefix("quality_delta."),           QualityRegression, Blocking),

    // Contract breaches (policy violations)
    entry(Prefix("boundary."),               ContractBreak,      Blocking),
    entry(Exact("exception.expired"),        ContractBreak,      Blocking),
    entry(Exact("exception.budget_exceeded"), ContractBreak,     Blocking),

    // Observations (tracked, not individually blocking)
    entry(Prefix("loc."),                    ContractBreak,      Observation),
    entry(Prefix("surface."),               ContractBreak,      Observation),
    entry(Prefix("duplicates."),            ContractBreak,      Observation),
    entry(Prefix("env_registry."),          ContractBreak,      Observation),
    entry(Prefix("tool_budget."),           ContractBreak,      Observation),

    // Gate tool execution
    entry(Prefix("gate.receipt_contract"),   RuntimeRisk,        Blocking),
    entry(Prefix("gate.tool_failed"),       ContractBreak,      Blocking),
    entry(Prefix("gate.run_failed"),        RuntimeRisk,        Blocking),
    entry(Prefix("gate."),                  SchemaConfig,       Blocking),
    entry(Prefix("witness."),              RuntimeRisk,         Blocking),

    // Fallback: Unknown + Blocking (fail-closed)
];
```

NOTE: Suffix entries for `.check_failed`/`.read_failed`/`.stat_failed` are matched BEFORE prefix entries for `loc.`/`surface.`/etc. This ensures `loc.read_failed` is RuntimeRisk+Blocking, not ContractBreak+Observation.

### 4.2 Error Classes

```rust
pub enum ErrorClass {
    SchemaConfig,       // config parse/structure errors
    ContractBreak,      // policy breach, business test failure
    RuntimeRisk,        // check infrastructure failure
    Security,           // supply_chain, allow_any policy
    QualityRegression,  // quality_delta.* violations
    TransientTool,      // runner timeout/IO error ONLY (not test failures)
    Unknown,            // unrecognized -> fail-closed -> Blocked
}
```

### 4.3 Decision Algorithm

```rust
pub(crate) fn decide(reasons: &[DecisionReason]) -> DecisionStatus {
    let blocking: Vec<_> = reasons.iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .collect();

    if blocking.is_empty() {
        return DecisionStatus::Pass;
    }

    let has_hard_block = blocking.iter().any(|r| !matches!(r.class, ErrorClass::TransientTool));

    if has_hard_block {
        DecisionStatus::Blocked
    } else {
        DecisionStatus::Retryable
    }
}
```

### 4.4 TransientTool Classification

TransientTool = **only** infrastructure failures of the runner, NOT business failures:

- Timeout -> TransientTool (Retryable)
- Runner IO error (could not spawn process) -> TransientTool (Retryable)
- Test exit code != 0 -> ContractBreak (Blocked) -- the test ran and found a problem
- Lint exit code != 0 -> ContractBreak (Blocked) -- the linter ran and found a problem

### 4.5 Receipt Contract

Per-tool receipt contract validates that execution actually happened, but does NOT override the tool's own pass/fail:

```toml
# In tool.toml:
[tool.receipt_contract]
min_duration_ms = 1000
min_stdout_bytes = 100
expect_stdout_pattern = "test result:"
```

Receipt contract violation = separate `gate.receipt_contract_violated` violation (RuntimeRisk, Blocking). The tool's exit code still determines `receipt.success`.

### 4.6 Registry Exhaustiveness Test

```rust
#[test]
fn all_builtin_codes_classified_as_non_unknown() {
    let known_codes = [
        "config.parse_failed", "config.plugins_dir_missing", "config.empty",
        "config.quality_contract_missing", "config.threshold_weakened",
        "config.mandatory_check_removed",
        "boundary.rule_violation", "boundary.check_failed",
        "loc.max_exceeded", "loc.read_failed", "loc.check_failed",
        "surface.max_exceeded", "surface.check_failed",
        "duplicates.found", "duplicates.stat_failed", "duplicates.read_failed",
        "env_registry.unregistered_usage", "env_registry.required_missing",
        "env_registry.registry_missing", "env_registry.registry_invalid",
        "supply_chain.lockfile_missing", "supply_chain.prerelease_dependency",
        "supply_chain.read_failed", "supply_chain.manifest_parse_failed",
        "tool_budget.max_tools_total_exceeded", "tool_budget.max_tools_per_plugin_exceeded",
        "tool_budget.max_gate_tools_exceeded", "tool_budget.max_checks_total_exceeded",
        "exception.allowlist_invalid", "exception.expired", "exception.budget_exceeded",
        "failure_modes.invalid",
        "quality_delta.trust_regression", "quality_delta.trust_below_minimum",
        "quality_delta.coverage_regression", "quality_delta.risk_profile_regression",
        "quality_delta.loc_regression", "quality_delta.surface_regression",
        "quality_delta.duplicates_regression", "quality_delta.scope_narrowed",
        "quality_delta.config_changed", "quality_delta.check_failed",
        "gate.empty_sequence", "gate.duplicate_tool_id", "gate.unknown_tool_id",
        "gate.tool_failed", "gate.run_failed", "gate.receipt_invariant_failed",
        "gate.validate_failed", "gate.receipt_contract_violated",
        "witness.write_failed", "witness.rotation_failed",
        "security.allow_any_policy",
    ];
    for code in &known_codes {
        let (class, _tier) = classify(code);
        assert_ne!(class, ErrorClass::Unknown,
            "built-in code {code} must not classify as Unknown");
    }
}

#[test]
fn unknown_code_is_unknown() {
    let (class, tier) = classify("something.never.seen");
    assert_eq!(class, ErrorClass::Unknown);
    assert_eq!(tier, ViolationTier::Blocking);
}
```

---

## 5. Quality Delta

### 5.1 Unified Quality Snapshot

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySnapshot {
    pub version: u32,  // 1

    // Holistic posture (raw, pre-suppress)
    pub trust_score: i32,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: BTreeMap<String, usize>,

    // Granular ratchets (replaces loc.json, surface.json, duplicates.json)
    pub loc_per_file: BTreeMap<String, usize>,
    pub surface_items: Vec<String>,           // sorted
    pub duplicate_groups: Vec<Vec<String>>,     // sorted groups of sorted paths

    // Scope tracking (stable denominator)
    pub file_universe: FileUniverse,

    // Provenance
    pub written_at: String,                     // ISO 8601
    pub written_by: Option<BaselineMaintenance>,
    pub config_hash: String,                    // SHA256 of canonicalized config
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileUniverse {
    pub loc_universe: usize,
    pub loc_scanned: usize,
    pub surface_universe: usize,
    pub surface_scanned: usize,
    pub boundary_universe: usize,
    pub boundary_scanned: usize,
    pub duplicates_universe: usize,
    pub duplicates_scanned: usize,
}
```

All maps are `BTreeMap` for deterministic serialization. Atomic writes via tmp+rename.

Schema migration: `version > SUPPORTED_MAX` -> fail-closed. `version < CURRENT` -> migration function or fail-closed if migration not possible.

### 5.2 Comparison Algorithm

Ratchet checks (in quality_delta, all produce Blocking violations):

| Check | Violation code | What |
|---|---|---|
| Trust regression | `quality_delta.trust_regression` | raw trust_score < snapshot |
| Trust floor | `quality_delta.trust_below_minimum` | trust_score < min_trust_score |
| Coverage regression | `quality_delta.coverage_regression` | coverage_covered < snapshot (integer, not %) |
| Risk profile | `quality_delta.risk_profile_regression` | weighted_risk increase > max_allowed |
| LOC regression | `quality_delta.loc_regression` | any file's LOC grew vs snapshot |
| Surface regression | `quality_delta.surface_regression` | new public items added vs snapshot |
| Duplicates regression | `quality_delta.duplicates_regression` | new duplicate groups vs snapshot |
| Scope narrowing | `quality_delta.scope_narrowed` | scan_ratio = scanned/universe dropped per domain |
| Config changed | `quality_delta.config_changed` | canonicalized config hash differs from snapshot |

### 5.3 Scope Narrowing (Stable Denominator)

```rust
fn check_scope_narrowing(baseline: &FileUniverse, current: &FileUniverse, max: f64, out: &mut Vec<Violation>) {
    let domains = [
        ("loc", baseline.loc_scanned, baseline.loc_universe,
                current.loc_scanned, current.loc_universe),
        ("surface", baseline.surface_scanned, baseline.surface_universe,
                    current.surface_scanned, current.surface_universe),
        ("boundary", baseline.boundary_scanned, baseline.boundary_universe,
                     current.boundary_scanned, current.boundary_universe),
        ("duplicates", baseline.duplicates_scanned, baseline.duplicates_universe,
                       current.duplicates_scanned, current.duplicates_universe),
    ];
    for (domain, base_scanned, base_universe, curr_scanned, curr_universe) in domains {
        if base_universe == 0 || curr_universe == 0 { continue; }
        let base_ratio = base_scanned as f64 / base_universe as f64;
        let curr_ratio = curr_scanned as f64 / curr_universe as f64;
        let drop = base_ratio - curr_ratio;
        if drop > max {
            // scan_ratio dropped: likely include_globs narrowed or exclude_globs widened
            out.push(violation("quality_delta.scope_narrowed", ...));
        }
    }
}
```

### 5.4 Baseline Write Guard

- `write_baseline=true` + `mode=ratchet` without `baseline_maintenance` -> error
- `write_baseline=true` + `mode=ratchet` + `baseline_maintenance` (reason >= 20 chars, owner) -> allowed, recorded in snapshot + witness
- `write_baseline=true` + `mode=strict|warn` -> always allowed (onboarding flow)

### 5.5 First-Run Behavior

No snapshot exists -> pass silently. Underlying checks still enforce. Agent must explicitly write baseline to activate ratchet.

### 5.6 Migration from Legacy Baselines

On first run with quality_delta configured:
1. If `quality_snapshot.json` missing but `loc.json`/`public_surface.json`/`duplicates.json` exist
2. Load legacy baselines, build QualitySnapshot from their data + current trust/coverage/risk
3. Write `quality_snapshot.json`
4. Legacy files NOT deleted (grace period)

---

## 6. Witness Chain

### 6.1 Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChainEntry {
    pub gate_kind: String,
    pub timestamp: String,      // ISO 8601
    pub witness_sha256: String, // SHA256 of the witness JSON
    pub prev_hash: String,      // SHA256 of previous chain entry (or "genesis")
    pub entry_hash: String,     // SHA256(prev_hash + witness_sha256 + timestamp + gate_kind)
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChain {
    pub entries: Vec<WitnessChainEntry>,
}
```

### 6.2 Operations

- Gate runner appends entry after witness write
- On gate start: load chain, verify last entry's `entry_hash` matches recomputed value
- Chain stored at `.agents/mcp/compas/witness/chain.json`
- Append-only: entries never removed (separate from witness file rotation)

---

## 7. quality_contract.toml

```toml
[quality]
min_trust_score = 60
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

[autonomy]
max_files_per_step = 5        # advisory, future use
max_loc_delta_per_step = 500  # advisory, future use

[proof]
require_witness = true        # real gate runs only, dry-run bypassed

[exceptions]
max_exceptions = 10
max_suppressed_ratio = 0.30
max_exception_window_days = 90

[receipt_defaults]
min_duration_ms = 500
min_stdout_bytes = 10

[governance]
mandatory_checks = ["boundary", "supply_chain"]
mandatory_failure_modes = ["security_baseline", "resilience_defaults"]
min_failure_modes = 8
config_hash = "sha256:..."    # computed from canonicalized config model

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
```

Loading: in `load_repo_config()`. Missing file = `config.quality_contract_missing`:
- ratchet/strict -> Blocking
- warn -> Observation (diagnostic for onboarding)

---

## 8. Anti-Gaming Measures

### 8.1 P0 (Complete system bypass -- Slice 1)

| Vector | Measure |
|---|---|
| Baseline reset via write_baseline | Baseline write guard: write+ratchet requires maintenance window |
| AllowAny plugin | Critical finding `security.allow_any_policy` |
| No-op gate tools | Per-tool receipt contract |
| Witness tampering | Witness chain with prev_hash+entry_hash |

### 8.2 P1 (High-impact manipulations -- Slice 2-3)

| Vector | Measure |
|---|---|
| Code to unchecked paths | scope_narrowed via scan_ratio tracking |
| Severity trade-off | risk_profile_regression via weighted_risk |
| Allowlist proliferation | Exception budget check |
| Threshold weakening | Config hash lock |
| Check removal | Mandatory checks enforcement |
| Catalog shrinking | Mandatory failure modes + min catalog size |
| Coverage inflation | Check effectiveness rules |

### 8.3 P2 (Semantic -- Slice 4)

| Vector | Measure |
|---|---|
| Dead code | Gate tool: `cargo check` with `#[deny(dead_code)]` |
| Error swallowing | Boundary rules for `Err(_) =>` patterns |
| File splitting evasion | New-file-in-gated-directory auto-include |
| Test quality | Coverage delta tracking |

---

## 9. Test Strategy

### 9.1 Unit Tests

- Judge: classification exhaustiveness, decision monotonicity, all known codes != Unknown
- quality_delta: snapshot roundtrip, each regression type, first-run pass, migration
- Registry: table completeness

### 9.2 Property-Based Tests

- Decision monotonicity: adding reasons never makes status more permissive
- Classification totality: any string produces a valid ErrorClass
- Verdict serialization roundtrip
- Action plan covers every reason code

### 9.3 Gaming Scenario Tests

- Severity shift: fix 5 lows, introduce 1 high -> blocked by risk_profile_regression
- Coverage stripping: remove checks -> blocked by coverage_regression
- Critical introduction: trade mediums for critical -> blocked by trust_regression + risk_profile
- Baseline reset attempt: write_baseline in ratchet without maintenance -> error
- Allowlist flood: >10 entries -> exception.budget_exceeded
- Scope narrowing: narrow include_globs -> scope_narrowed

### 9.4 Contract Tests (Schema v3)

- MCP smoke test: v2 consumer ignores new fields, still works
- CLI smoke test: v3 output round-trips through JSON
- Verdict presence: validate and gate always produce verdict in schema v3

### 9.5 Gate Tests (CI)

- `cargo test -p ai-dx-mcp`
- `cargo run -p ai-dx-mcp -- validate ratchet`
- `./dx ci-fast --dry-run`
- New: quality_delta gaming scenarios
- New: witness chain verification

---

## 10. Rollout Slices

### Slice 1 -- Foundation: Judge + API v3 + quality_contract

New: `judge.rs`, `judge/registry.rs`, quality_contract.toml loading, API v3 types, witness chain.
Changed: `api.rs`, `app.rs`, `gate_runner.rs`, `config.rs`, `repo.rs`, `witness.rs`, `init/planner.rs`.
Verdict present in output but `ok` uses old logic. P0 anti-gaming active.

### Slice 2 -- Quality Delta (unified ratchet)

New: `checks/quality_delta.rs`, `QualitySnapshot`, migration from legacy baselines.
Changed: `checks/loc.rs` (remove ratchet), `checks/surface.rs` (remove ratchet), `checks/duplicates.rs` (remove ratchet), `app.rs` (Phase-2 wiring).
quality_delta blocking enforcement active. Legacy baselines preserved.

### Slice 3 -- Hardening + Security pack

New: `checks/exception_budget.rs`, security gate tools.
Changed: `validate_insights.rs` (effectiveness rules), `failure_modes.rs` (mandatory modes).
Config lock, mandatory checks, exception budget active. Legacy baselines cleanup.

### Slice 4 -- Semantic integrity

New boundary rules, dead code gate tool, test coverage tracking.
Closes P2 anti-gaming vectors.

---

## 11. Done Criteria

| # | Criterion | Slice |
|---|---|---|
| 1 | Verdict (pass/retryable/blocked) in every validate/gate output | 1 |
| 2 | quality_posture (raw) and trust_score (display) separated in API | 1 |
| 3 | P0 anti-gaming: baseline guard, AllowAny block, receipt contract, witness chain | 1 |
| 4 | quality_delta: unified ratchet for trust+coverage+risk+loc+surface+duplicates | 2 |
| 5 | Per-check ratchet removed, loc/surface/duplicates = Observation | 2 |
| 6 | Gaming scenarios (severity shift, coverage strip, critical trade) blocked | 2 |
| 7 | Exception budget, mandatory checks, config lock | 3 |
| 8 | Security tooling in gate pipeline | 3 |
| 9 | Dead code + error swallowing + scope gap detection | 4 |
