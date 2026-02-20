# Compas Sentinel v1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make compas an invariant quality gate with anti-gaming protection: verdict-based decisions, unified quality_delta ratchet, witness chain, and table-driven violation classification.

**Architecture:** Two-phase validate (Phase-1: existing checks → raw insights; Phase-2: quality_delta against baseline snapshot) + judge module (table-driven classification → verdict). quality_delta replaces per-check ratchet in loc/surface/duplicates. Witness chain with cryptographic prev_hash linking.

**Tech Stack:** Rust, serde/serde_json, sha2, BTreeMap for deterministic serialization, toml for quality_contract, tempfile for tests.

---

## Plan Delta v1.1 (approved corrections, authoritative)

> This section has priority over older wording below if conflicts appear.

1. **Targeting rule:** references are by `file + symbol` (line numbers are illustrative and non-authoritative).
2. **Judge layout:** use `crates/ai-dx-mcp/src/judge/mod.rs` + `crates/ai-dx-mcp/src/judge/registry.rs` only (no parallel `src/judge.rs`).
3. **Dataflow contract:** `raw (pre-suppress)` is used for `quality_delta` and `quality_posture`; `display (post-suppress)` is used for user-facing trust/risk/coverage.
4. **Retryable policy:** `TransientTool` is only runner/infra/transient failures (timeouts, spawn/io failures). Tool business failures (non-zero exit from test/lint/etc.) are **not** transient.
5. **Receipt contract semantics:** validates execution evidence, but never overrides tool success semantics.
6. **Witness chain semantics:** hash-chain is mandatory (`prev_hash` + `entry_hash`); runtime path must not rely on `eprintln!` for failure handling.
7. **Scope narrowing metric:** compare per-domain `scan_ratio = scanned / universe`, not raw universe size shrink.
8. **Config lock hash:** hash canonical normalized config model (serde model), not raw TOML text.
9. **Schema v3 compatibility:** keep explicit contract tests for legacy consumers.
10. **Duplicate detection policy:** 
   - `tools.duplicate_exact` (blocking, high precision only),
   - `tools.duplicate_semantic` (observation in v1, non-blocking).
11. **Scope control:** deep semantic integrity checks (dead code / error swallowing heuristics) stay in Sentinel v1.2 unless required to unblock v1 invariants.
12. **Decision/ok matrix (explicit):**
   - `validate`: `ok = (blocking_count == 0) || mode == warn`; `Retryable` is not emitted in validate path.
   - `gate`: `Pass` if no blocking, `Retryable` only for pure transient runner failures, otherwise `Blocked`.
   - strict/ratchet never treat `Retryable` as success.

---

## Slice 1: Foundation — Judge + API v3 + quality_contract

### Task 1: Add ViolationTier enum and tier field to Violation

**Files:**
- Modify: `crates/ai-dx-mcp/src/api.rs:22-27` (Violation struct)
- Modify: `crates/ai-dx-mcp/src/api.rs:1` (add imports)

**Step 1: Write failing test for ViolationTier serialization**

Add to `crates/ai-dx-mcp/src/api.rs` in the `#[cfg(test)] mod tests` block (after line 259):

```rust
#[test]
fn violation_tier_default_is_blocking() {
    let v: Violation = serde_json::from_value(serde_json::json!({
        "code": "test.x",
        "message": "msg"
    }))
    .expect("deserialize Violation without tier");
    assert_eq!(v.tier, ViolationTier::Blocking);
}

#[test]
fn violation_tier_roundtrip() {
    let v = Violation {
        code: "test.x".to_string(),
        message: "msg".to_string(),
        path: None,
        details: None,
        tier: ViolationTier::Observation,
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["tier"], "observation");
    let back: Violation = serde_json::from_value(json).unwrap();
    assert_eq!(back.tier, ViolationTier::Observation);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ai-dx-mcp api::tests::violation_tier 2>&1 | head -30`
Expected: compilation error — `ViolationTier` not found, `tier` field missing.

**Step 3: Implement ViolationTier and add tier to Violation**

In `crates/ai-dx-mcp/src/api.rs`, add after the `ValidateMode` enum (after line 35):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViolationTier {
    Blocking,
    Observation,
}

impl Default for ViolationTier {
    fn default() -> Self {
        Self::Blocking
    }
}
```

Then modify the `Violation` struct to add the `tier` field with a default:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Violation {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub details: Option<serde_json::Value>,
    #[serde(default)]
    pub tier: ViolationTier,
}
```

**Step 4: Fix all existing Violation construction sites**

Do **not** rely on a hand-written file list. Make this exhaustive and compiler-driven:

1. Add constructors in `api.rs`:
   - `Violation::blocking(...)`
   - `Violation::observation(...)`
2. Replace all production `Violation { ... }` literals with these constructors.
3. Verify no stray construction sites remain:
   - `rg -n 'Violation\\s*\\{' crates/ai-dx-mcp/src`
   - Expected matches: only `struct Violation` definition (and explicitly allowed test fixtures, if any).
4. Keep tier assignment centralized: observations only where policy says so (loc/surface/duplicates observation paths).

**Step 5: Run all tests to verify nothing breaks**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all tests pass, including the new `violation_tier_*` tests.

**Step 6: Commit**

```bash
git add crates/ai-dx-mcp/src/api.rs crates/ai-dx-mcp/src/app.rs \
  crates/ai-dx-mcp/src/checks/ crates/ai-dx-mcp/src/exceptions.rs \
  crates/ai-dx-mcp/src/packs/
git commit -m "feat(sentinel): add ViolationTier enum with default Blocking"
```

---

### Task 2: Add API v3 types — QualityPosture, Decision, Verdict

**Files:**
- Modify: `crates/ai-dx-mcp/src/api.rs` (add new types)
- Modify: `crates/ai-dx-mcp/src/api/insights.rs` (no changes needed, but reference)

**Step 1: Write failing test for Verdict roundtrip**

Add to `crates/ai-dx-mcp/src/api.rs` tests:

```rust
#[test]
fn verdict_roundtrip() {
    let v = Verdict {
        decision: Decision {
            status: DecisionStatus::Blocked,
            reasons: vec![DecisionReason {
                code: "boundary.rule_violation".to_string(),
                class: ErrorClass::ContractBreak,
                tier: ViolationTier::Blocking,
            }],
            blocking_count: 1,
            observation_count: 0,
        },
        quality_posture: None,
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["decision"]["status"], "blocked");
    let back: Verdict = serde_json::from_value(json).unwrap();
    assert_eq!(back.decision.status, DecisionStatus::Blocked);
}

#[test]
fn quality_posture_roundtrip() {
    let qp = QualityPosture {
        trust_score: 85,
        trust_grade: "B".to_string(),
        coverage_covered: 8,
        coverage_total: 10,
        weighted_risk: 12,
        findings_total: 3,
        risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)]
            .into_iter()
            .collect(),
    };
    let json = serde_json::to_value(&qp).unwrap();
    let back: QualityPosture = serde_json::from_value(json).unwrap();
    assert_eq!(back.trust_score, 85);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ai-dx-mcp api::tests::verdict_roundtrip 2>&1 | head -20`
Expected: compilation error — types not defined.

**Step 3: Add the types in api.rs**

After the `ViolationTier` enum, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Pass,
    Retryable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    SchemaConfig,
    ContractBreak,
    RuntimeRisk,
    Security,
    QualityRegression,
    TransientTool,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecisionReason {
    pub code: String,
    pub class: ErrorClass,
    pub tier: ViolationTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Decision {
    pub status: DecisionStatus,
    pub reasons: Vec<DecisionReason>,
    pub blocking_count: usize,
    pub observation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualityPosture {
    pub trust_score: i32,
    pub trust_grade: String,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Verdict {
    pub decision: Decision,
    pub quality_posture: Option<QualityPosture>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BaselineMaintenance {
    pub reason: String,
    pub owner: String,
}
```

Add these new types to the `pub use` in the api module or make them accessible.

**Step 4: Run tests**

Run: `cargo test -p ai-dx-mcp api::tests 2>&1 | tail -10`
Expected: all pass.

**Step 5: Commit**

```bash
git add crates/ai-dx-mcp/src/api.rs
git commit -m "feat(sentinel): add API v3 types — Verdict, Decision, QualityPosture, ErrorClass"
```

---

### Task 3: Add verdict and quality_posture to ValidateOutput and GateOutput

**Files:**
- Modify: `crates/ai-dx-mcp/src/api.rs:97-118` (ValidateOutput)
- Modify: `crates/ai-dx-mcp/src/api.rs:246-256` (GateOutput)
- Modify: `crates/ai-dx-mcp/src/app.rs:218-238` (ValidateOutput construction)
- Modify: `crates/ai-dx-mcp/src/app.rs:34-50` (error ValidateOutput)
- Modify: `crates/ai-dx-mcp/src/gate_runner.rs:18-28` (gate_fail)
- Modify: `crates/ai-dx-mcp/src/gate_runner.rs:185-195` (ok GateOutput)
- Modify: `crates/ai-dx-mcp/src/witness.rs:215-235` (test GateOutput)
- Modify: `crates/ai-dx-mcp/tests/mcp_smoke.rs` (assertions)

**Step 1: Write failing test for schema_version "3"**

Add test in `crates/ai-dx-mcp/src/api.rs` tests:

```rust
#[test]
fn validate_output_schema_v3_has_verdict_and_posture() {
    let output = ValidateOutput {
        ok: true,
        error: None,
        schema_version: "3".to_string(),
        repo_root: ".".to_string(),
        mode: ValidateMode::Warn,
        violations: vec![],
        findings_v2: vec![],
        suppressed: vec![],
        loc: None,
        boundary: None,
        public_surface: None,
        effective_config: None,
        risk_summary: None,
        coverage: None,
        trust_score: None,
        verdict: Some(Verdict {
            decision: Decision {
                status: DecisionStatus::Pass,
                reasons: vec![],
                blocking_count: 0,
                observation_count: 0,
            },
            quality_posture: None,
        }),
        quality_posture: None,
    };
    let json = serde_json::to_value(&output).unwrap();
    assert_eq!(json["schema_version"], "3");
    assert!(json["verdict"].is_object());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ai-dx-mcp api::tests::validate_output_schema_v3 2>&1 | head -20`
Expected: compilation error — `verdict` and `quality_posture` fields missing.

**Step 3: Add fields to ValidateOutput and GateOutput**

In `ValidateOutput`, add after `trust_score`:

```rust
pub verdict: Option<Verdict>,
pub quality_posture: Option<QualityPosture>,
```

In `GateOutput`, add after `witness`:

```rust
pub verdict: Option<Verdict>,
```

**Step 4: Fix all construction sites**

Add `verdict: None, quality_posture: None` to every `ValidateOutput { ... }` literal:
- `crates/ai-dx-mcp/src/app.rs:34-50` (error path) — add `verdict: None, quality_posture: None`
- `crates/ai-dx-mcp/src/app.rs:218-238` (success path) — add `verdict: None, quality_posture: None`

Add `verdict: None` to every `GateOutput { ... }` literal:
- `crates/ai-dx-mcp/src/gate_runner.rs:18-28` (gate_fail) — add `verdict: None`
- `crates/ai-dx-mcp/src/gate_runner.rs:185-195` (success) — add `verdict: None`

Fix test ValidateOutput/GateOutput constructions:
- `crates/ai-dx-mcp/src/witness.rs:215-235` — add `verdict: None, quality_posture: None` to ValidateOutput and `verdict: None` to GateOutput

**Step 5: Bump schema_version to "3"**

In `crates/ai-dx-mcp/src/app.rs`, change both occurrences of `schema_version: "2".to_string()` to `schema_version: "3".to_string()`.

**Step 6: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass. The MCP smoke test should still work since new fields are `Option` (backward compat).

**Step 7: Commit**

```bash
git add crates/ai-dx-mcp/src/api.rs crates/ai-dx-mcp/src/app.rs \
  crates/ai-dx-mcp/src/gate_runner.rs crates/ai-dx-mcp/src/witness.rs \
  crates/ai-dx-mcp/tests/
git commit -m "feat(sentinel): add verdict/quality_posture to output, bump schema to v3"
```

---

### Task 4: Add BaselineMaintenance to ValidateRequest

**Files:**
- Modify: `crates/ai-dx-mcp/src/api.rs:37-44` (ValidateRequest)
- Modify: `crates/ai-dx-mcp/src/app.rs:30` (validate signature)
- Modify: `crates/ai-dx-mcp/src/server.rs:35-43` (validate handler)
- Modify: `crates/ai-dx-mcp/src/gate_runner.rs:79` (validate call)

**Step 1: Write failing test**

In `crates/ai-dx-mcp/src/api.rs` tests:

```rust
#[test]
fn validate_request_with_baseline_maintenance() {
    let req: ValidateRequest = serde_json::from_value(serde_json::json!({
        "mode": "ratchet",
        "write_baseline": true,
        "baseline_maintenance": {
            "reason": "Quarterly baseline refresh after major refactor",
            "owner": "team-lead"
        }
    }))
    .expect("deserialize with baseline_maintenance");
    assert!(req.baseline_maintenance.is_some());
    let bm = req.baseline_maintenance.unwrap();
    assert_eq!(bm.owner, "team-lead");
}

#[test]
fn validate_request_without_baseline_maintenance_still_works() {
    let req: ValidateRequest = serde_json::from_value(serde_json::json!({
        "mode": "warn"
    }))
    .expect("deserialize without baseline_maintenance");
    assert!(req.baseline_maintenance.is_none());
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp api::tests::validate_request_with_baseline 2>&1 | head -20`
Expected: `deny_unknown_fields` rejects `baseline_maintenance` since the field doesn't exist yet.

**Step 3: Add field to ValidateRequest**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateRequest {
    pub repo_root: Option<String>,
    pub mode: ValidateMode,
    pub write_baseline: Option<bool>,
    #[serde(default)]
    pub baseline_maintenance: Option<BaselineMaintenance>,
}
```

**Step 4: Update validate() signature to accept baseline_maintenance**

In `crates/ai-dx-mcp/src/app.rs`, change:
```rust
pub fn validate(repo_root: &str, mode: ValidateMode, write_baseline: bool) -> ValidateOutput {
```
to:
```rust
pub fn validate(
    repo_root: &str,
    mode: ValidateMode,
    write_baseline: bool,
    baseline_maintenance: Option<&BaselineMaintenance>,
) -> ValidateOutput {
```

Update call sites:
- `crates/ai-dx-mcp/src/server.rs:38` — pass `params.0.baseline_maintenance.as_ref()`
- `crates/ai-dx-mcp/src/gate_runner.rs:79` — pass `None` (gate always validates without writing baseline)

**Step 5: Add baseline write guard logic**

In `crates/ai-dx-mcp/src/app.rs`, at the start of `validate()`, after loading config, add guard:

```rust
// Baseline write guard: ratchet + write_baseline requires maintenance window
if write_baseline && matches!(mode, ValidateMode::Ratchet) {
    match &baseline_maintenance {
        None => {
            return ValidateOutput {
                ok: false,
                error: Some(ApiError {
                    code: "config.baseline_write_requires_maintenance".to_string(),
                    message: "write_baseline=true in ratchet mode requires baseline_maintenance with reason (>=20 chars) and owner".to_string(),
                }),
                schema_version: "3".to_string(),
                // ... fill defaults ...
            };
        }
        Some(bm) if bm.reason.trim().len() < 20 => {
            return ValidateOutput {
                ok: false,
                error: Some(ApiError {
                    code: "config.baseline_maintenance_reason_too_short".to_string(),
                    message: format!("baseline_maintenance.reason must be >=20 chars (got {})", bm.reason.trim().len()),
                }),
                schema_version: "3".to_string(),
                // ... fill defaults ...
            };
        }
        Some(_) => { /* valid maintenance window */ }
    }
}
```

**Step 6: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass.

**Step 7: Commit**

```bash
git add crates/ai-dx-mcp/src/api.rs crates/ai-dx-mcp/src/app.rs \
  crates/ai-dx-mcp/src/server.rs crates/ai-dx-mcp/src/gate_runner.rs
git commit -m "feat(sentinel): add BaselineMaintenance + baseline write guard"
```

---

### Task 5: Create judge module — registry.rs with table-driven classification

**Files:**
- Create: `crates/ai-dx-mcp/src/judge/mod.rs`
- Create: `crates/ai-dx-mcp/src/judge/registry.rs`
- Modify: `crates/ai-dx-mcp/src/lib.rs` (add `pub mod judge;`)

**Step 1: Write failing tests for classification**

Create `crates/ai-dx-mcp/src/judge/registry.rs` with tests first:

```rust
// crates/ai-dx-mcp/src/judge/registry.rs

use crate::api::{ErrorClass, ViolationTier};

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

// Will be filled in Step 3
pub static VIOLATION_REGISTRY: &[ViolationClassEntry] = &[];

pub fn classify(code: &str) -> (ErrorClass, ViolationTier) {
    // Suffix rules first (higher specificity), then Exact, then Prefix
    // First pass: Suffix
    for entry in VIOLATION_REGISTRY {
        if let ViolationPattern::Suffix(s) = &entry.pattern {
            if code.ends_with(s) {
                return (entry.class, entry.tier);
            }
        }
    }
    // Second pass: Exact
    for entry in VIOLATION_REGISTRY {
        if let ViolationPattern::Exact(s) = &entry.pattern {
            if code == *s {
                return (entry.class, entry.tier);
            }
        }
    }
    // Third pass: Prefix
    for entry in VIOLATION_REGISTRY {
        if let ViolationPattern::Prefix(s) = &entry.pattern {
            if code.starts_with(s) {
                return (entry.class, entry.tier);
            }
        }
    }
    // Fallback: Unknown + Blocking (fail-closed)
    (ErrorClass::Unknown, ViolationTier::Blocking)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_ne!(
                class,
                ErrorClass::Unknown,
                "built-in code {code} must not classify as Unknown"
            );
        }
    }

    #[test]
    fn unknown_code_is_unknown() {
        let (class, tier) = classify("something.never.seen");
        assert_eq!(class, ErrorClass::Unknown);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn suffix_has_priority_over_prefix() {
        // loc.read_failed should match Suffix(".read_failed") -> RuntimeRisk+Blocking
        // NOT Prefix("loc.") -> ContractBreak+Observation
        let (class, tier) = classify("loc.read_failed");
        assert_eq!(class, ErrorClass::RuntimeRisk);
        assert_eq!(tier, ViolationTier::Blocking);
    }

    #[test]
    fn observation_tier_for_loc_surface_duplicates() {
        let codes = ["loc.max_exceeded", "surface.max_exceeded", "duplicates.found"];
        for code in &codes {
            let (_class, tier) = classify(code);
            assert_eq!(
                tier,
                ViolationTier::Observation,
                "{code} should be Observation"
            );
        }
    }

    #[test]
    fn quality_delta_is_blocking() {
        let (class, tier) = classify("quality_delta.trust_regression");
        assert_eq!(class, ErrorClass::QualityRegression);
        assert_eq!(tier, ViolationTier::Blocking);
    }
}
```

**Step 2: Create judge/mod.rs stub**

Create `crates/ai-dx-mcp/src/judge/mod.rs`:

```rust
pub mod registry;
```

**Step 3: Run tests to verify they fail**

Run: `cargo test -p ai-dx-mcp judge::registry::tests 2>&1 | head -30`
Expected: `all_builtin_codes_classified_as_non_unknown` fails because registry is empty.

**Step 4: Fill VIOLATION_REGISTRY**

In `registry.rs`, replace the empty `VIOLATION_REGISTRY` with the full table:

```rust
const fn entry(
    pattern: ViolationPattern,
    class: ErrorClass,
    tier: ViolationTier,
) -> ViolationClassEntry {
    ViolationClassEntry {
        pattern,
        class,
        tier,
    }
}

use ViolationPattern::*;
use ErrorClass::*;
use ViolationTier::*;

pub static VIOLATION_REGISTRY: &[ViolationClassEntry] = &[
    // Infrastructure failures (suffix rules — matched first for priority)
    entry(Suffix(".check_failed"),            RuntimeRisk,       Blocking),
    entry(Suffix(".read_failed"),             RuntimeRisk,       Blocking),
    entry(Suffix(".stat_failed"),             RuntimeRisk,       Blocking),
    entry(Suffix(".manifest_parse_failed"),   RuntimeRisk,       Blocking),

    // Config / structural (exact + prefix)
    entry(Prefix("config."),                  SchemaConfig,      Blocking),
    entry(Prefix("failure_modes."),           SchemaConfig,      Blocking),
    entry(Prefix("pack."),                    SchemaConfig,      Blocking),
    entry(Exact("exception.allowlist_invalid"), SchemaConfig,    Blocking),

    // Security
    entry(Prefix("supply_chain."),            Security,          Blocking),
    entry(Exact("security.allow_any_policy"), Security,          Blocking),

    // Quality regression
    entry(Prefix("quality_delta."),           QualityRegression, Blocking),

    // Contract breaches
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
];
```

NOTE: The `const fn` and use of `Copy` types in `ErrorClass`/`ViolationTier` makes this work in a static context. `ViolationPattern` uses `&'static str` which is fine.

Add `#[derive(Copy)]` to `ViolationPattern`, `ErrorClass`, `ViolationTier` if not already.

**Step 5: Add judge module to lib.rs**

In `crates/ai-dx-mcp/src/lib.rs`, add:
```rust
pub mod judge;
```

**Step 6: Run tests**

Run: `cargo test -p ai-dx-mcp judge::registry::tests 2>&1 | tail -15`
Expected: all 5 tests pass.

**Step 7: Commit**

```bash
git add crates/ai-dx-mcp/src/judge/ crates/ai-dx-mcp/src/lib.rs crates/ai-dx-mcp/src/api.rs
git commit -m "feat(sentinel): add judge registry with table-driven classification"
```

---

### Task 6: Add judge decision algorithm — decide() and judge_validate()

**Files:**
- Modify: `crates/ai-dx-mcp/src/judge/mod.rs`

**Step 1: Write failing tests for decide()**

Add to `crates/ai-dx-mcp/src/judge/mod.rs`:

```rust
pub mod registry;

use crate::api::{DecisionReason, DecisionStatus, ErrorClass, Verdict, Decision, ViolationTier, Violation, QualityPosture, ValidateMode};
use registry::classify;

pub fn decide_gate(reasons: &[DecisionReason]) -> DecisionStatus {
    todo!()
}

pub fn decide_validate(reasons: &[DecisionReason], mode: ValidateMode) -> DecisionStatus {
    todo!()
}

pub fn judge_validate(violations: &[Violation], mode: ValidateMode) -> Verdict {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_blocking_yields_pass() {
        let reasons = vec![
            DecisionReason {
                code: "loc.max_exceeded".to_string(),
                class: ErrorClass::ContractBreak,
                tier: ViolationTier::Observation,
            },
        ];
        assert_eq!(decide_gate(&reasons), DecisionStatus::Pass);
    }

    #[test]
    fn transient_only_yields_retryable() {
        let reasons = vec![
            DecisionReason {
                code: "gate.run_failed".to_string(),
                class: ErrorClass::TransientTool,
                tier: ViolationTier::Blocking,
            },
        ];
        assert_eq!(decide_gate(&reasons), DecisionStatus::Retryable);
    }

    #[test]
    fn any_hard_block_yields_blocked() {
        let reasons = vec![
            DecisionReason {
                code: "gate.run_failed".to_string(),
                class: ErrorClass::TransientTool,
                tier: ViolationTier::Blocking,
            },
            DecisionReason {
                code: "boundary.rule_violation".to_string(),
                class: ErrorClass::ContractBreak,
                tier: ViolationTier::Blocking,
            },
        ];
        assert_eq!(decide_gate(&reasons), DecisionStatus::Blocked);
    }

    #[test]
    fn empty_reasons_yields_pass() {
        assert_eq!(decide_gate(&[]), DecisionStatus::Pass);
    }

    #[test]
    fn monotonicity_adding_reason_never_softens() {
        let base = vec![
            DecisionReason {
                code: "boundary.rule_violation".to_string(),
                class: ErrorClass::ContractBreak,
                tier: ViolationTier::Blocking,
            },
        ];
        let status_before = decide_gate(&base);
        let mut extended = base.clone();
        extended.push(DecisionReason {
            code: "loc.max_exceeded".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Observation,
        });
        let status_after = decide_gate(&extended);
        // Blocked should not become Pass or Retryable
        assert!(severity_ord(status_after) >= severity_ord(status_before));
    }

    fn severity_ord(s: DecisionStatus) -> u8 {
        match s {
            DecisionStatus::Pass => 0,
            DecisionStatus::Retryable => 1,
            DecisionStatus::Blocked => 2,
        }
    }

    #[test]
    fn judge_validate_classifies_violations() {
        let violations = vec![
            Violation {
                code: "loc.max_exceeded".to_string(),
                message: "too big".to_string(),
                path: Some("x.rs".to_string()),
                details: None,
                tier: ViolationTier::Blocking, // will be reclassified
            },
            Violation {
                code: "boundary.rule_violation".to_string(),
                message: "forbidden".to_string(),
                path: Some("y.rs".to_string()),
                details: None,
                tier: ViolationTier::Blocking,
            },
        ];
        let verdict = judge_validate(&violations, ValidateMode::Ratchet);
        // loc.* is Observation, boundary.* is Blocking
        assert_eq!(verdict.decision.observation_count, 1);
        assert_eq!(verdict.decision.blocking_count, 1);
        assert_eq!(verdict.decision.status, DecisionStatus::Blocked);
    }

    #[test]
    fn validate_mode_warn_never_returns_retryable() {
        let reasons = vec![DecisionReason {
            code: "boundary.rule_violation".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Blocking,
        }];
        assert_eq!(decide_validate(&reasons, ValidateMode::Warn), DecisionStatus::Pass);
        assert_eq!(decide_validate(&reasons, ValidateMode::Ratchet), DecisionStatus::Blocked);
    }
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp judge::tests 2>&1 | head -20`
Expected: `todo!()` panics.

**Step 3: Implement decide() and judge_validate()**

```rust
pub fn decide_gate(reasons: &[DecisionReason]) -> DecisionStatus {
    let blocking: Vec<&DecisionReason> = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .collect();

    if blocking.is_empty() {
        return DecisionStatus::Pass;
    }

    let has_hard_block = blocking
        .iter()
        .any(|r| !matches!(r.class, ErrorClass::TransientTool));

    if has_hard_block {
        DecisionStatus::Blocked
    } else {
        DecisionStatus::Retryable
    }
}

pub fn decide_validate(reasons: &[DecisionReason], mode: ValidateMode) -> DecisionStatus {
    let blocking_count = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .count();
    if blocking_count == 0 || matches!(mode, ValidateMode::Warn) {
        DecisionStatus::Pass
    } else {
        // validate path is deterministic: no Retryable status
        DecisionStatus::Blocked
    }
}

pub fn judge_validate(violations: &[Violation]) -> Verdict {
    let mut reasons: Vec<DecisionReason> = Vec::with_capacity(violations.len());

    for v in violations {
        let (class, tier) = classify(&v.code);
        reasons.push(DecisionReason {
            code: v.code.clone(),
            class,
            tier,
        });
    }

    let blocking_count = reasons.iter().filter(|r| r.tier == ViolationTier::Blocking).count();
    let observation_count = reasons.iter().filter(|r| r.tier == ViolationTier::Observation).count();
    let status = decide_validate(&reasons, ValidateMode::Ratchet);

    Verdict {
        decision: Decision {
            status,
            reasons,
            blocking_count,
            observation_count,
        },
        quality_posture: None,
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ai-dx-mcp judge::tests 2>&1 | tail -15`
Expected: all pass.

**Step 5: Commit**

```bash
git add crates/ai-dx-mcp/src/judge/
git commit -m "feat(sentinel): add judge decide() + judge_validate() with monotonicity"
```

---

### Task 7: Wire judge into app.rs validate flow

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs:214-238` (after allowlist, before return)

**Step 1: Write integration test**

Create `crates/ai-dx-mcp/tests/judge_integration.rs`:

```rust
use ai_dx_mcp::api::{DecisionStatus, ValidateMode};

#[test]
fn validate_warn_returns_verdict_pass() {
    let repo_root = {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir.parent().unwrap().parent().unwrap().to_path_buf()
    };
    let output = ai_dx_mcp::app::validate(
        &repo_root.to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );
    assert_eq!(output.schema_version, "3");
    let verdict = output.verdict.expect("verdict must be present in v3");
    // In warn mode, even with violations, verdict should still classify them
    assert!(verdict.decision.reasons.len() >= 0);
    // Since it's warn mode, ok should be true regardless
    assert!(output.ok);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p ai-dx-mcp --test judge_integration 2>&1 | head -20`
Expected: `verdict` is `None` because we haven't wired it yet.

**Step 3: Wire judge into validate()**

In `crates/ai-dx-mcp/src/app.rs`, after the allowlist application (line 214) and before constructing `ValidateOutput`, add:

```rust
// Phase 1 contract:
// - raw insights (pre-suppress) feed quality_posture + quality_delta
// - display insights (post-suppress) feed user-facing trust/risk/coverage

// Phase 3: Judge classifies final blocking surface from final violations set
// (for Slice 1 this is post-suppress Phase-1 violations only)
let verdict = crate::judge::judge_validate(&suppression.violations, mode);

// validate contract:
// - warn  => ok=true
// - strict/ratchet => ok only when verdict=Pass
let ok = matches!(mode, ValidateMode::Warn)
    || matches!(verdict.decision.status, DecisionStatus::Pass);
```

Then update the `ValidateOutput` construction to use the new `ok` and add `verdict: Some(verdict)`.

**Step 4: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass. The ok logic may differ from before (some violations that were Observation won't block anymore), which is the desired behavior.

**Step 5: Commit**

```bash
git add crates/ai-dx-mcp/src/app.rs crates/ai-dx-mcp/tests/judge_integration.rs
git commit -m "feat(sentinel): wire judge into validate flow, ok now considers ViolationTier"
```

---

### Task 8: Load quality_contract.toml

**Files:**
- Modify: `crates/ai-dx-mcp/src/config.rs` (add QualityContractConfig)
- Modify: `crates/ai-dx-mcp/src/repo.rs:17-24` (add to RepoConfig)
- Modify: `crates/ai-dx-mcp/src/repo.rs:26-290` (load quality_contract.toml)
- Create: `.agents/mcp/compas/quality_contract.toml` (default config)

**Step 1: Write failing test for config parsing**

Add to `crates/ai-dx-mcp/src/config.rs` tests:

```rust
#[test]
fn quality_contract_deserialize() {
    let s = r#"
[quality]
min_trust_score = 60
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

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

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
"#;
    let cfg: QualityContractConfig = toml::from_str(s).expect("parse quality_contract");
    assert_eq!(cfg.quality.min_trust_score, 60);
    assert!(!cfg.quality.allow_trust_drop);
    assert_eq!(cfg.governance.mandatory_checks, vec!["boundary", "supply_chain"]);
    assert!((cfg.baseline.max_scope_narrowing - 0.10).abs() < f64::EPSILON);
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp config::tests::quality_contract 2>&1 | head -20`
Expected: `QualityContractConfig` not found.

**Step 3: Add QualityContractConfig to config.rs**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityContractConfig {
    #[serde(default)]
    pub quality: QualityThresholds,
    #[serde(default)]
    pub exceptions: ExceptionLimits,
    #[serde(default)]
    pub receipt_defaults: ReceiptDefaults,
    #[serde(default)]
    pub governance: GovernanceConfig,
    #[serde(default)]
    pub baseline: BaselineConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityThresholds {
    #[serde(default = "default_min_trust_score")]
    pub min_trust_score: i32,
    #[serde(default)]
    pub allow_trust_drop: bool,
    #[serde(default)]
    pub allow_coverage_drop: bool,
    #[serde(default)]
    pub max_weighted_risk_increase: i32,
}

fn default_min_trust_score() -> i32 { 60 }

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            min_trust_score: 60,
            allow_trust_drop: false,
            allow_coverage_drop: false,
            max_weighted_risk_increase: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionLimits {
    #[serde(default = "default_max_exceptions")]
    pub max_exceptions: usize,
    #[serde(default = "default_max_suppressed_ratio")]
    pub max_suppressed_ratio: f64,
    #[serde(default = "default_max_exception_window_days")]
    pub max_exception_window_days: u32,
}

fn default_max_exceptions() -> usize { 10 }
fn default_max_suppressed_ratio() -> f64 { 0.30 }
fn default_max_exception_window_days() -> u32 { 90 }

impl Default for ExceptionLimits {
    fn default() -> Self {
        Self {
            max_exceptions: 10,
            max_suppressed_ratio: 0.30,
            max_exception_window_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDefaults {
    #[serde(default = "default_min_duration_ms")]
    pub min_duration_ms: u64,
    #[serde(default = "default_min_stdout_bytes")]
    pub min_stdout_bytes: usize,
}

fn default_min_duration_ms() -> u64 { 500 }
fn default_min_stdout_bytes() -> usize { 10 }

impl Default for ReceiptDefaults {
    fn default() -> Self {
        Self {
            min_duration_ms: 500,
            min_stdout_bytes: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernanceConfig {
    #[serde(default)]
    pub mandatory_checks: Vec<String>,
    #[serde(default)]
    pub mandatory_failure_modes: Vec<String>,
    #[serde(default = "default_min_failure_modes")]
    pub min_failure_modes: usize,
    pub config_hash: Option<String>,
}

fn default_min_failure_modes() -> usize { 8 }

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            mandatory_checks: vec![],
            mandatory_failure_modes: vec![],
            min_failure_modes: 8,
            config_hash: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineConfig {
    #[serde(default = "default_snapshot_path")]
    pub snapshot_path: String,
    #[serde(default = "default_max_scope_narrowing")]
    pub max_scope_narrowing: f64,
}

fn default_snapshot_path() -> String {
    ".agents/mcp/compas/baselines/quality_snapshot.json".to_string()
}
fn default_max_scope_narrowing() -> f64 { 0.10 }

impl Default for BaselineConfig {
    fn default() -> Self {
        Self {
            snapshot_path: default_snapshot_path(),
            max_scope_narrowing: 0.10,
        }
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ai-dx-mcp config::tests 2>&1 | tail -10`
Expected: all pass.

**Step 5: Add quality_contract to RepoConfig and loading**

In `crates/ai-dx-mcp/src/repo.rs`, add `quality_contract: Option<QualityContractConfig>` to `RepoConfig`.

In `load_repo_config()`, after loading plugins (before `Ok(RepoConfig {...})`), add:

```rust
let quality_contract_path = repo_root.join(".agents/mcp/compas/quality_contract.toml");
let quality_contract = if quality_contract_path.is_file() {
    let raw = fs::read_to_string(&quality_contract_path)
        .map_err(|e| RepoConfigError::ReadPlugin {
            path: quality_contract_path.clone(),
            source: e,
        })?;
    let parsed: crate::config::QualityContractConfig =
        toml::from_str(&raw).map_err(|e| RepoConfigError::ParsePlugin {
            path: quality_contract_path.clone(),
            message: e.to_string(),
        })?;
    Some(parsed)
} else {
    None
};
```

Add `quality_contract` to the returned `RepoConfig`.

**Step 6: Create default quality_contract.toml**

```toml
# .agents/mcp/compas/quality_contract.toml
# Compas Sentinel v1 — quality policy contract

[quality]
min_trust_score = 60
allow_trust_drop = false
allow_coverage_drop = false
max_weighted_risk_increase = 0

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

[baseline]
snapshot_path = ".agents/mcp/compas/baselines/quality_snapshot.json"
max_scope_narrowing = 0.10
```

**Step 7: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass.

**Step 8: Commit**

```bash
git add crates/ai-dx-mcp/src/config.rs crates/ai-dx-mcp/src/repo.rs \
  .agents/mcp/compas/quality_contract.toml
git commit -m "feat(sentinel): add quality_contract.toml config + loading"
```

---

### Task 9: Witness chain — append-only with prev_hash

**Files:**
- Modify: `crates/ai-dx-mcp/src/witness.rs` (add WitnessChain logic)

**Step 1: Write failing test for chain append + verify**

Add to `crates/ai-dx-mcp/src/witness.rs` tests:

```rust
#[test]
fn witness_chain_append_and_verify() {
    let dir = tempfile::tempdir().unwrap();
    let chain_path = dir.path().join("chain.json");

    // First entry — genesis
    let entry1 = append_chain_entry(
        &chain_path,
        "ci-fast",
        "abc123def456",
        true,
    )
    .unwrap();
    assert_eq!(entry1.prev_hash, "genesis");
    assert!(!entry1.entry_hash.is_empty());

    // Second entry — links to first
    let entry2 = append_chain_entry(
        &chain_path,
        "ci-fast",
        "def456abc789",
        true,
    )
    .unwrap();
    assert_eq!(entry2.prev_hash, entry1.entry_hash);

    // Verify chain
    let chain = load_witness_chain(&chain_path).unwrap();
    assert_eq!(chain.entries.len(), 2);
    assert!(verify_chain_integrity(&chain));
}

#[test]
fn witness_chain_detects_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let chain_path = dir.path().join("chain.json");

    append_chain_entry(&chain_path, "ci-fast", "aaa", true).unwrap();
    append_chain_entry(&chain_path, "ci-fast", "bbb", true).unwrap();

    // Tamper: modify first entry's hash
    let mut chain = load_witness_chain(&chain_path).unwrap();
    chain.entries[0].entry_hash = "tampered".to_string();
    let json = serde_json::to_string_pretty(&chain).unwrap();
    std::fs::write(&chain_path, json).unwrap();

    let chain = load_witness_chain(&chain_path).unwrap();
    assert!(!verify_chain_integrity(&chain));
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp witness::tests::witness_chain 2>&1 | head -20`
Expected: functions not found.

**Step 3: Implement witness chain**

Add to `crates/ai-dx-mcp/src/witness.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChainEntry {
    pub gate_kind: String,
    pub timestamp: String,
    pub witness_sha256: String,
    pub prev_hash: String,
    pub entry_hash: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessChain {
    pub entries: Vec<WitnessChainEntry>,
}

fn compute_entry_hash(prev_hash: &str, witness_sha256: &str, timestamp: &str, gate_kind: &str) -> String {
    let input = format!("{prev_hash}:{witness_sha256}:{timestamp}:{gate_kind}");
    sha256_hex(input.as_bytes())
}

pub(crate) fn load_witness_chain(path: &Path) -> Result<WitnessChain, std::io::Error> {
    if !path.is_file() {
        return Ok(WitnessChain { entries: vec![] });
    }
    let raw = std::fs::read_to_string(path)?;
    let chain: WitnessChain = serde_json::from_str(&raw)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(chain)
}

pub(crate) fn verify_chain_integrity(chain: &WitnessChain) -> bool {
    let mut expected_prev = "genesis".to_string();
    for entry in &chain.entries {
        if entry.prev_hash != expected_prev {
            return false;
        }
        let computed = compute_entry_hash(
            &entry.prev_hash,
            &entry.witness_sha256,
            &entry.timestamp,
            &entry.gate_kind,
        );
        if entry.entry_hash != computed {
            return false;
        }
        expected_prev = entry.entry_hash.clone();
    }
    true
}

pub(crate) fn append_chain_entry(
    chain_path: &Path,
    gate_kind: &str,
    witness_sha256: &str,
    ok: bool,
) -> Result<WitnessChainEntry, std::io::Error> {
    let mut chain = load_witness_chain(chain_path)?;

    let prev_hash = chain
        .entries
        .last()
        .map(|e| e.entry_hash.clone())
        .unwrap_or_else(|| "genesis".to_string());

    let timestamp = chrono::Utc::now().to_rfc3339();
    let entry_hash = compute_entry_hash(&prev_hash, witness_sha256, &timestamp, gate_kind);

    let entry = WitnessChainEntry {
        gate_kind: gate_kind.to_string(),
        timestamp,
        witness_sha256: witness_sha256.to_string(),
        prev_hash,
        entry_hash,
        ok,
    };

    chain.entries.push(entry.clone());

    if let Some(parent) = chain_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Atomic write: tmp + rename
    let tmp_path = chain_path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&chain)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, chain_path)?;

    Ok(entry)
}
```

**Step 4: Wire chain append into maybe_write_gate_witness**

After the existing witness write (line 141), before rotation, add:

```rust
// Append to witness chain
let chain_path = repo_root.join(".agents/mcp/compas/witness/chain.json");
if let Err(e) = append_chain_entry(
    &chain_path,
    gate_kind_slug(kind),
    &sha256_hex(bytes),
    out.ok,
) {
    // Runtime path must avoid ad-hoc stderr logging.
    // Emit structured violation so judge can classify deterministically.
    receipt_violations.push(Violation {
        code: "witness.chain_append_failed".to_string(),
        message: format!("failed to append witness chain: {e}"),
        path: Some(chain_path.to_string_lossy().to_string()),
        details: None,
        tier: ViolationTier::Blocking,
    });
}
```

**Step 5: Run tests**

Run: `cargo test -p ai-dx-mcp witness::tests 2>&1 | tail -15`
Expected: all pass.

**Step 6: Commit**

```bash
git add crates/ai-dx-mcp/src/witness.rs
git commit -m "feat(sentinel): add witness chain with prev_hash+entry_hash linking"
```

---

### Task 10: AllowAny detection — security.allow_any_policy violation

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs` (add AllowAny check)
- Modify: `crates/ai-dx-mcp/src/config.rs:42-47` (make ToolExecutionPolicyMode pub)

**Step 1: Write failing test**

Create `crates/ai-dx-mcp/tests/allow_any_detection.rs`:

```rust
use ai_dx_mcp::api::ValidateMode;

#[test]
fn allow_any_plugin_produces_security_violation() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();

    // Bootstrap a plugin with AllowAny
    let plugin_dir = repo_root.join(".agents/mcp/compas/plugins/dangerous");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
id = "dangerous"
description = "A plugin that allows any command execution"

[tool_policy]
mode = "allow_any"

[[tools]]
id = "danger-tool"
description = "Runs anything"
command = "echo"
args = ["hello"]
"#,
    )
    .unwrap();

    let output = ai_dx_mcp::app::validate(
        &repo_root.to_string_lossy(),
        ValidateMode::Warn,
        false,
        None,
    );

    assert!(
        output.violations.iter().any(|v| v.code == "security.allow_any_policy"),
        "should produce security.allow_any_policy violation"
    );
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp --test allow_any_detection 2>&1 | head -20`
Expected: no such violation produced.

**Step 3: Add AllowAny detection in validate()**

In `crates/ai-dx-mcp/src/app.rs`, after loading config and before running checks, add:

```rust
// P0 Anti-gaming: detect AllowAny plugins
for (plugin_id, _plugin) in &cfg.plugins {
    // Check if plugin uses AllowAny policy (need access to raw plugin config)
    // Since tool_policy is already validated in repo.rs, we need to surface it
}
```

The cleanest approach: in `crates/ai-dx-mcp/src/repo.rs`, add `allow_any_plugins: Vec<String>` to `RepoConfig`. During plugin loading, if `tool_policy.mode == ToolExecutionPolicyMode::AllowAny`, push the plugin_id to this vec.

Then in `app.rs`:
```rust
for plugin_id in &cfg.allow_any_plugins {
    violations.push(Violation {
        code: "security.allow_any_policy".to_string(),
        message: format!("plugin {plugin_id} uses AllowAny execution policy — this bypasses all tool execution safety"),
        path: None,
        details: None,
        tier: ViolationTier::Blocking,
    });
}
```

**Step 4: Run tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass.

**Step 5: Commit**

```bash
git add crates/ai-dx-mcp/src/app.rs crates/ai-dx-mcp/src/repo.rs \
  crates/ai-dx-mcp/src/config.rs crates/ai-dx-mcp/tests/allow_any_detection.rs
git commit -m "feat(sentinel): detect AllowAny plugins as security.allow_any_policy"
```

---

### Task 11: Schema v3 contract tests

**Files:**
- Modify: `crates/ai-dx-mcp/tests/mcp_smoke.rs` (add v3 assertions)

**Step 1: Add v3 contract assertions to MCP smoke test**

In `crates/ai-dx-mcp/tests/mcp_smoke.rs`, after existing validate assertions (around line 137), add:

```rust
// v3 contract: schema_version is "3" and verdict is present
assert_eq!(validate.schema_version, "3");
// v2 consumer compat: ok still works as boolean
assert!(validate.ok || !validate.ok); // just assert it's present

// New: verdict must be present in v3
let verdict = validate.verdict.as_ref().expect("verdict must be present in schema v3");
// Decision status is one of pass/retryable/blocked
let status = &verdict.decision.status;
assert!(
    matches!(status, ai_dx_mcp::api::DecisionStatus::Pass | ai_dx_mcp::api::DecisionStatus::Retryable | ai_dx_mcp::api::DecisionStatus::Blocked),
    "unexpected decision status: {status:?}"
);
```

**Step 2: Run MCP smoke test**

Run: `cargo test -p ai-dx-mcp --test mcp_smoke 2>&1 | tail -20`
Expected: pass.

**Step 3: Commit**

```bash
git add crates/ai-dx-mcp/tests/mcp_smoke.rs
git commit -m "test(sentinel): add schema v3 contract assertions to MCP smoke test"
```

---

## Slice 2: Quality Delta — Unified Ratchet

### Task 12: Create quality_delta module with QualitySnapshot

**Files:**
- Create: `crates/ai-dx-mcp/src/checks/quality_delta.rs`
- Modify: `crates/ai-dx-mcp/src/checks/mod.rs` (add module)

**Step 1: Write failing test for snapshot roundtrip**

Create `crates/ai-dx-mcp/src/checks/quality_delta.rs`:

```rust
use crate::api::Violation;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySnapshot {
    pub version: u32,
    pub trust_score: i32,
    pub coverage_covered: usize,
    pub coverage_total: usize,
    pub weighted_risk: i32,
    pub findings_total: usize,
    pub risk_by_severity: BTreeMap<String, usize>,
    pub loc_per_file: BTreeMap<String, usize>,
    pub surface_items: Vec<String>,
    pub duplicate_groups: Vec<Vec<String>>,
    pub file_universe: FileUniverse,
    pub written_at: String,
    pub written_by: Option<crate::api::BaselineMaintenance>,
    pub config_hash: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> QualitySnapshot {
        QualitySnapshot {
            version: SNAPSHOT_VERSION,
            trust_score: 85,
            coverage_covered: 8,
            coverage_total: 10,
            weighted_risk: 12,
            findings_total: 3,
            risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)]
                .into_iter()
                .collect(),
            loc_per_file: [("src/main.rs".to_string(), 100)]
                .into_iter()
                .collect(),
            surface_items: vec!["src/api.rs::pub_fn:validate".to_string()],
            duplicate_groups: vec![],
            file_universe: FileUniverse {
                loc_universe: 50, loc_scanned: 45,
                surface_universe: 50, surface_scanned: 45,
                boundary_universe: 50, boundary_scanned: 50,
                duplicates_universe: 50, duplicates_scanned: 50,
            },
            written_at: "2026-02-17T12:00:00Z".to_string(),
            written_by: None,
            config_hash: "sha256:abc".to_string(),
        }
    }

    #[test]
    fn snapshot_roundtrip_deterministic() {
        let snap = sample_snapshot();
        let json1 = serde_json::to_string_pretty(&snap).unwrap();
        let parsed: QualitySnapshot = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string_pretty(&parsed).unwrap();
        assert_eq!(json1, json2, "serialization must be deterministic (BTreeMap)");
    }

    #[test]
    fn snapshot_version_checked() {
        let mut snap = sample_snapshot();
        snap.version = 999;
        let json = serde_json::to_string(&snap).unwrap();
        let err = load_snapshot_from_str(&json);
        assert!(err.is_err(), "unknown version should fail-closed");
    }
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp checks::quality_delta::tests 2>&1 | head -20`
Expected: `load_snapshot_from_str` not found.

**Step 3: Implement load/save snapshot**

```rust
pub fn load_snapshot_from_str(json: &str) -> Result<QualitySnapshot, String> {
    let snap: QualitySnapshot =
        serde_json::from_str(json).map_err(|e| format!("failed to parse quality snapshot: {e}"))?;
    if snap.version > SNAPSHOT_VERSION {
        return Err(format!(
            "quality snapshot version {} > supported max {}",
            snap.version, SNAPSHOT_VERSION
        ));
    }
    Ok(snap)
}

pub fn load_snapshot(path: &Path) -> Result<Option<QualitySnapshot>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read quality snapshot {:?}: {e}", path))?;
    load_snapshot_from_str(&raw).map(Some)
}

pub fn write_snapshot(path: &Path, snapshot: &QualitySnapshot) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create snapshot dir {:?}: {e}", parent))?;
    }
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("failed to serialize snapshot: {e}"))?;
    // Atomic write: tmp + rename
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)
        .map_err(|e| format!("failed to write snapshot tmp {:?}: {e}", tmp))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("failed to rename snapshot {:?}: {e}", path))?;
    Ok(())
}
```

**Step 4: Add module to checks/mod.rs**

```rust
pub mod quality_delta;
```

**Step 5: Run tests**

Run: `cargo test -p ai-dx-mcp checks::quality_delta::tests 2>&1 | tail -10`
Expected: all pass.

**Step 6: Commit**

```bash
git add crates/ai-dx-mcp/src/checks/quality_delta.rs crates/ai-dx-mcp/src/checks/mod.rs
git commit -m "feat(sentinel): add QualitySnapshot struct with deterministic serialization"
```

---

### Task 13: Implement quality_delta comparison algorithm

**Files:**
- Modify: `crates/ai-dx-mcp/src/checks/quality_delta.rs`

**Step 1: Write failing tests for each ratchet check**

Add to the tests module in `quality_delta.rs`:

```rust
#[test]
fn trust_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.trust_score = baseline.trust_score - 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.trust_regression"));
}

#[test]
fn trust_below_minimum_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.trust_score = 30; // below min_trust_score=60
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.trust_below_minimum"));
}

#[test]
fn coverage_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.coverage_covered = baseline.coverage_covered - 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.coverage_regression"));
}

#[test]
fn risk_profile_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.weighted_risk = baseline.weighted_risk + 1;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.risk_profile_regression"));
}

#[test]
fn loc_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.loc_per_file.insert("src/main.rs".to_string(), 200); // was 100
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.loc_regression"));
}

#[test]
fn surface_regression_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.surface_items.push("src/api.rs::pub_fn:new_thing".to_string());
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.surface_regression"));
}

#[test]
fn scope_narrowing_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    // Drop loc scan ratio from 45/50=0.90 to 20/50=0.40 — a 0.50 drop > 0.10 threshold
    current.file_universe.loc_scanned = 20;
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.scope_narrowed"));
}

#[test]
fn config_changed_detected() {
    let baseline = sample_snapshot();
    let mut current = sample_snapshot();
    current.config_hash = "sha256:different".to_string();
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.config_changed"));
}

#[test]
fn no_regressions_yields_empty() {
    let baseline = sample_snapshot();
    let current = sample_snapshot();
    let violations = compare(&baseline, &current, &default_contract());
    assert!(violations.is_empty(), "identical snapshot should produce no violations: {:?}", violations);
}

fn default_contract() -> crate::config::QualityContractConfig {
    toml::from_str("").unwrap()
}
```

**Step 2: Run to verify fails**

Run: `cargo test -p ai-dx-mcp checks::quality_delta::tests::trust_regression 2>&1 | head -20`
Expected: `compare` not found.

**Step 3: Implement compare()**

```rust
use crate::api::ViolationTier;
use crate::config::QualityContractConfig;

pub fn compare(
    baseline: &QualitySnapshot,
    current: &QualitySnapshot,
    contract: &QualityContractConfig,
) -> Vec<Violation> {
    let mut violations = Vec::new();

    // 1. Trust regression
    if !contract.quality.allow_trust_drop && current.trust_score < baseline.trust_score {
        violations.push(Violation {
            code: "quality_delta.trust_regression".to_string(),
            message: format!(
                "trust score regressed: baseline={}, current={}",
                baseline.trust_score, current.trust_score
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 2. Trust floor
    if current.trust_score < contract.quality.min_trust_score {
        violations.push(Violation {
            code: "quality_delta.trust_below_minimum".to_string(),
            message: format!(
                "trust score {} below minimum {}",
                current.trust_score, contract.quality.min_trust_score
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 3. Coverage regression
    if !contract.quality.allow_coverage_drop && current.coverage_covered < baseline.coverage_covered {
        violations.push(Violation {
            code: "quality_delta.coverage_regression".to_string(),
            message: format!(
                "coverage regressed: baseline={}, current={}",
                baseline.coverage_covered, current.coverage_covered
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 4. Risk profile regression
    let risk_increase = current.weighted_risk - baseline.weighted_risk;
    if risk_increase > contract.quality.max_weighted_risk_increase {
        violations.push(Violation {
            code: "quality_delta.risk_profile_regression".to_string(),
            message: format!(
                "weighted risk increased: baseline={}, current={}, increase={}, max_allowed={}",
                baseline.weighted_risk, current.weighted_risk, risk_increase, contract.quality.max_weighted_risk_increase
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 5. LOC regression (per-file)
    for (path, current_loc) in &current.loc_per_file {
        if let Some(base_loc) = baseline.loc_per_file.get(path) {
            if current_loc > base_loc {
                violations.push(Violation {
                    code: "quality_delta.loc_regression".to_string(),
                    message: format!(
                        "LOC grew: {} baseline={} current={}",
                        path, base_loc, current_loc
                    ),
                    path: Some(path.clone()),
                    details: None,
                    tier: ViolationTier::Blocking,
                });
            }
        }
    }

    // 6. Surface regression
    let baseline_set: std::collections::BTreeSet<_> = baseline.surface_items.iter().collect();
    let new_items: Vec<_> = current.surface_items.iter()
        .filter(|item| !baseline_set.contains(item))
        .collect();
    if !new_items.is_empty() {
        violations.push(Violation {
            code: "quality_delta.surface_regression".to_string(),
            message: format!("new public surface items: {} added", new_items.len()),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 7. Duplicates regression
    let baseline_dup_set: std::collections::BTreeSet<Vec<String>> = baseline.duplicate_groups.iter().cloned().collect();
    let new_dup_groups: Vec<_> = current.duplicate_groups.iter()
        .filter(|g| !baseline_dup_set.contains(*g))
        .collect();
    if !new_dup_groups.is_empty() {
        violations.push(Violation {
            code: "quality_delta.duplicates_regression".to_string(),
            message: format!("new duplicate groups: {} added", new_dup_groups.len()),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    // 8. Scope narrowing (per-domain scan_ratio)
    check_scope_narrowing(
        &baseline.file_universe,
        &current.file_universe,
        contract.baseline.max_scope_narrowing,
        &mut violations,
    );

    // 9. Config changed
    if baseline.config_hash != current.config_hash {
        violations.push(Violation {
            code: "quality_delta.config_changed".to_string(),
            message: format!(
                "config hash changed: baseline={}, current={}",
                baseline.config_hash, current.config_hash
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }

    violations
}

fn check_scope_narrowing(
    baseline: &FileUniverse,
    current: &FileUniverse,
    max: f64,
    out: &mut Vec<Violation>,
) {
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
        if base_universe == 0 || curr_universe == 0 {
            continue;
        }
        let base_ratio = base_scanned as f64 / base_universe as f64;
        let curr_ratio = curr_scanned as f64 / curr_universe as f64;
        let drop = base_ratio - curr_ratio;
        if drop > max {
            out.push(Violation {
                code: "quality_delta.scope_narrowed".to_string(),
                message: format!(
                    "scan ratio dropped for {domain}: baseline={:.2}, current={:.2}, drop={:.2}, max={:.2}",
                    base_ratio, curr_ratio, drop, max
                ),
                path: None,
                details: None,
                tier: ViolationTier::Blocking,
            });
        }
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p ai-dx-mcp checks::quality_delta::tests 2>&1 | tail -20`
Expected: all pass.

**Step 5: Commit**

```bash
git add crates/ai-dx-mcp/src/checks/quality_delta.rs
git commit -m "feat(sentinel): implement quality_delta comparison algorithm (9 ratchet checks)"
```

---

### Task 14: Remove per-check ratchet from loc.rs, surface.rs, duplicates.rs

**Files:**
- Modify: `crates/ai-dx-mcp/src/checks/loc.rs:56-197` — remove ratchet branches
- Modify: `crates/ai-dx-mcp/src/checks/surface.rs:160-244` — remove ratchet branches
- Modify: `crates/ai-dx-mcp/src/checks/duplicates.rs:150-298` — remove ratchet branches
- Modify: `crates/ai-dx-mcp/src/app.rs` — remove mode_ratchet/write_baseline from check calls

**Step 1: Simplify loc.rs**

Remove `mode_ratchet` and `write_baseline` params from `run_loc_check`. Remove baseline loading, ratchet comparison, and baseline writing. Keep only the `max_loc` threshold check. The function becomes:

```rust
pub fn run_loc_check(
    repo_root: &Path,
    cfg: &LocCheckConfigV2,
) -> Result<LocCheckResult, String> {
    // ... keep include/exclude globset build ...
    // ... keep file scan loop ...
    // ... keep max_loc threshold check (without baseline exceptions) ...
    // Remove: baseline loading, ratchet_regression check, baseline writing
    Ok(LocCheckResult {
        violations,
        files_scanned: files.len(),
        max_loc,
        worst_path,
        loc_per_file: files,  // NEW: expose for quality_delta
    })
}
```

Add `pub loc_per_file: BTreeMap<String, usize>` to `LocCheckResult`.

**Step 2: Simplify surface.rs**

Similarly, remove `mode_ratchet` and `write_baseline`. Keep only `max_items` check. Expose `current_items: BTreeSet<String>` in result.

**Step 3: Simplify duplicates.rs**

Remove `mode_ratchet` and `write_baseline`. Keep duplicate detection only. Expose `groups: BTreeMap<String, Vec<String>>` in result.

**Step 4: Update app.rs call sites**

Remove `mode_ratchet` and `write_baseline` from calls to `run_loc_check`, `run_surface_check`, `run_duplicates_check`. These functions no longer need ratchet parameters.

**Step 5: Update tests in each check module**

Remove ratchet-specific tests (they'll be covered by quality_delta tests). Keep threshold tests.

**Step 6: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass. Some existing ratchet tests will be removed or adapted.

**Step 7: Commit**

```bash
git add crates/ai-dx-mcp/src/checks/loc.rs crates/ai-dx-mcp/src/checks/surface.rs \
  crates/ai-dx-mcp/src/checks/duplicates.rs crates/ai-dx-mcp/src/app.rs
git commit -m "refactor(sentinel): remove per-check ratchet from loc/surface/duplicates"
```

---

### Task 15: Wire quality_delta into app.rs two-phase flow

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs` — add Phase-2 quality_delta call

**Step 1: Write failing integration test for quality_delta blocking**

Create `crates/ai-dx-mcp/tests/quality_delta_integration.rs`:

```rust
use ai_dx_mcp::api::ValidateMode;

#[test]
fn quality_delta_blocks_trust_regression() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();

    // Bootstrap minimal compas config
    // ... (create plugin.toml, quality_contract.toml, quality_snapshot.json) ...

    // Create a baseline with trust_score=85
    // Then modify code to produce lower trust score
    // Validate in ratchet mode should produce quality_delta.trust_regression

    // This is a complex integration test — see full implementation below
}
```

**Step 2: Build the Phase-2 flow in app.rs**

After Phase-1 (existing checks + allowlist + insights), add Phase-2:

```rust
// Phase-1 explicit split (must stay separate)
let phase1_raw_violations = violations_raw.clone();              // pre-suppress
let phase1_display_violations = suppression.violations.clone();  // post-suppress
let posture_raw = build_quality_posture(&findings_raw, &coverage_raw, &risk_raw);
let display_insights = build_display_insights(&phase1_display_violations);

// Phase 2: quality_delta (unified ratchet)
let quality_delta_violations = if matches!(mode, ValidateMode::Ratchet) {
    if let Some(contract) = &cfg.quality_contract {
        let snapshot_path = repo_root_path.join(&contract.baseline.snapshot_path);
        match crate::checks::quality_delta::load_snapshot(&snapshot_path) {
            Ok(Some(baseline)) => {
                let current = build_current_snapshot(
                    /* raw posture + raw check outputs only */
                    &posture_raw,
                    &raw_check_outputs,
                );
                crate::checks::quality_delta::compare(&baseline, &current, contract)
            }
            Ok(None) => vec![], // No snapshot = first run, pass silently
            Err(e) => vec![Violation {
                code: "quality_delta.check_failed".to_string(),
                message: e,
                path: None,
                details: None,
                tier: ViolationTier::Blocking,
            }],
        }
    } else {
        // No quality contract in ratchet mode = config.quality_contract_missing violation
        vec![Violation {
            code: "config.quality_contract_missing".to_string(),
            message: "quality_contract.toml is required in ratchet mode".to_string(),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        }]
    }
} else if matches!(mode, ValidateMode::Warn) && cfg.quality_contract.is_none() {
    // Warn mode without contract = diagnostic observation
    vec![Violation {
        code: "config.quality_contract_missing".to_string(),
        message: "quality_contract.toml not found (diagnostic)".to_string(),
        path: None,
        details: None,
        tier: ViolationTier::Observation,
    }]
} else {
    vec![]
};
```

The `build_current_snapshot` helper constructs `QualitySnapshot` from the raw insights + check results gathered in Phase-1.

**Step 3: Write baseline on write_baseline=true**

```rust
if write_baseline {
    if let Some(contract) = &cfg.quality_contract {
        let snapshot_path = repo_root_path.join(&contract.baseline.snapshot_path);
        let current = build_current_snapshot(/* ... */);
        if let Err(e) = crate::checks::quality_delta::write_snapshot(&snapshot_path, &current) {
            // Non-fatal in strict/warn, blocking error if write fails
            violations.push(Violation {
                code: "quality_delta.check_failed".to_string(),
                message: format!("failed to write quality snapshot: {e}"),
                path: None,
                details: None,
                tier: ViolationTier::Blocking,
            });
        }
    }
}
```

**Step 4: Combine violations for judge**

```rust
// Combine Phase-1 + Phase-2 violations
let mut all_violations = phase1_display_violations;
all_violations.extend(quality_delta_violations);

// Phase 3: Judge
let verdict = crate::judge::judge_validate(&all_violations, mode);

// Display fields come from post-suppress set; quality_posture comes from raw set
let trust_score_display = Some(display_insights.trust_score);
let quality_posture = Some(posture_raw);
```

**Step 5: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass.

**Step 6: Commit**

```bash
git add crates/ai-dx-mcp/src/app.rs crates/ai-dx-mcp/tests/quality_delta_integration.rs
git commit -m "feat(sentinel): wire quality_delta Phase-2 into validate flow"
```

---

### Task 16: Gaming scenario tests

**Files:**
- Create: `crates/ai-dx-mcp/tests/gaming_scenarios.rs`

**Step 1: Write gaming scenario tests**

```rust
//! Gaming scenario tests: verify that anti-gaming measures block manipulation attempts.

use ai_dx_mcp::checks::quality_delta::{QualitySnapshot, FileUniverse, compare};
use ai_dx_mcp::config::QualityContractConfig;

fn default_contract() -> QualityContractConfig {
    toml::from_str("").unwrap()
}

fn baseline() -> QualitySnapshot {
    QualitySnapshot {
        version: 1,
        trust_score: 85,
        coverage_covered: 8,
        coverage_total: 10,
        weighted_risk: 12,
        findings_total: 3,
        risk_by_severity: [("high".to_string(), 1), ("medium".to_string(), 2)].into_iter().collect(),
        loc_per_file: [("src/main.rs".to_string(), 100), ("src/lib.rs".to_string(), 50)].into_iter().collect(),
        surface_items: vec!["src/api.rs::pub_fn:validate".to_string()],
        duplicate_groups: vec![],
        file_universe: FileUniverse {
            loc_universe: 50, loc_scanned: 45,
            surface_universe: 50, surface_scanned: 45,
            boundary_universe: 50, boundary_scanned: 50,
            duplicates_universe: 50, duplicates_scanned: 50,
        },
        written_at: "2026-02-17T12:00:00Z".to_string(),
        written_by: None,
        config_hash: "sha256:abc".to_string(),
    }
}

#[test]
fn gaming_severity_shift_fix_lows_introduce_high() {
    let b = baseline();
    let mut c = baseline();
    // "Fix" 5 lows by removing findings, but introduce 1 high -> weighted risk goes up
    c.weighted_risk = b.weighted_risk + 5; // net worse
    c.trust_score = b.trust_score - 3;     // trust drops
    let violations = compare(&b, &c, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.trust_regression"));
    assert!(violations.iter().any(|v| v.code == "quality_delta.risk_profile_regression"));
}

#[test]
fn gaming_coverage_stripping() {
    let b = baseline();
    let mut c = baseline();
    c.coverage_covered = 5; // dropped from 8
    let violations = compare(&b, &c, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.coverage_regression"));
}

#[test]
fn gaming_scope_narrowing_via_excludes() {
    let b = baseline();
    let mut c = baseline();
    // Narrow scope to exclude files with violations
    c.file_universe.loc_scanned = 10; // was 45/50, now 10/50
    let violations = compare(&b, &c, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.scope_narrowed"));
}

#[test]
fn gaming_config_hash_tampering() {
    let b = baseline();
    let mut c = baseline();
    c.config_hash = "sha256:weakened_thresholds".to_string();
    let violations = compare(&b, &c, &default_contract());
    assert!(violations.iter().any(|v| v.code == "quality_delta.config_changed"));
}
```

**Step 2: Run tests**

Run: `cargo test -p ai-dx-mcp --test gaming_scenarios 2>&1 | tail -20`
Expected: all pass.

**Step 3: Commit**

```bash
git add crates/ai-dx-mcp/tests/gaming_scenarios.rs
git commit -m "test(sentinel): add gaming scenario tests for quality_delta anti-gaming"
```

---

### Task 17: Legacy baseline migration

**Files:**
- Modify: `crates/ai-dx-mcp/src/checks/quality_delta.rs`

**Step 1: Write failing test for migration**

```rust
#[test]
fn migrate_legacy_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path();
    let baselines = repo_root.join(".agents/mcp/compas/baselines");
    std::fs::create_dir_all(&baselines).unwrap();

    // Write legacy baselines
    std::fs::write(
        baselines.join("loc.json"),
        r#"{"files":{"src/main.rs":100}}"#,
    ).unwrap();
    std::fs::write(
        baselines.join("public_surface.json"),
        r#"{"items":["src/api.rs::pub_fn:validate"]}"#,
    ).unwrap();

    let snapshot = migrate_from_legacy(repo_root, 85, 8, 10, 12, "sha256:abc").unwrap();
    assert_eq!(snapshot.loc_per_file["src/main.rs"], 100);
    assert_eq!(snapshot.surface_items, vec!["src/api.rs::pub_fn:validate"]);
}
```

**Step 2: Implement migrate_from_legacy()**

```rust
pub fn migrate_from_legacy(
    repo_root: &Path,
    trust_score: i32,
    coverage_covered: usize,
    coverage_total: usize,
    weighted_risk: i32,
    config_hash: &str,
) -> Result<QualitySnapshot, String> {
    let baselines_dir = repo_root.join(".agents/mcp/compas/baselines");

    let loc_per_file = {
        let path = baselines_dir.join("loc.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::loc::LocBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline.files
        } else {
            BTreeMap::new()
        }
    };

    let surface_items = {
        let path = baselines_dir.join("public_surface.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::surface::SurfaceBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline.items
        } else {
            vec![]
        }
    };

    // duplicates: read if exists
    let duplicate_groups = {
        let path = baselines_dir.join("duplicates.json");
        if path.is_file() {
            let raw = std::fs::read_to_string(&path).map_err(|e| format!("{e}"))?;
            let baseline: crate::checks::duplicates::DuplicatesBaseline =
                serde_json::from_str(&raw).map_err(|e| format!("{e}"))?;
            baseline.groups.into_iter().map(|g| g.paths).collect()
        } else {
            vec![]
        }
    };

    Ok(QualitySnapshot {
        version: SNAPSHOT_VERSION,
        trust_score,
        coverage_covered,
        coverage_total,
        weighted_risk,
        findings_total: 0, // unknown from legacy
        risk_by_severity: BTreeMap::new(),
        loc_per_file,
        surface_items,
        duplicate_groups,
        file_universe: FileUniverse {
            loc_universe: 0, loc_scanned: 0,
            surface_universe: 0, surface_scanned: 0,
            boundary_universe: 0, boundary_scanned: 0,
            duplicates_universe: 0, duplicates_scanned: 0,
        },
        written_at: chrono::Utc::now().to_rfc3339(),
        written_by: None,
        config_hash: config_hash.to_string(),
    })
}
```

**Step 3: Run tests**

Run: `cargo test -p ai-dx-mcp checks::quality_delta::tests::migrate 2>&1 | tail -10`
Expected: pass.

**Step 4: Commit**

```bash
git add crates/ai-dx-mcp/src/checks/quality_delta.rs
git commit -m "feat(sentinel): add legacy baseline migration to unified QualitySnapshot"
```

---

## Slice 3: Hardening + Security Pack

### Task 18: Exception budget check

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs` — add exception budget enforcement after allowlist

**Step 1: Write failing test**

```rust
#[test]
fn exception_budget_exceeded() {
    // Create allowlist with 15 entries (exceeding max_exceptions=10)
    // Run validate in ratchet mode with quality_contract
    // Expect exception.budget_exceeded violation
}
```

**Step 2: Implement in app.rs**

After `apply_allowlist`, check the suppressed count against the quality contract's `max_exceptions`:

```rust
if let Some(contract) = &cfg.quality_contract {
    if suppression.suppressed.len() > contract.exceptions.max_exceptions {
        violations.push(Violation {
            code: "exception.budget_exceeded".to_string(),
            message: format!(
                "suppressed violations ({}) exceed max_exceptions ({})",
                suppression.suppressed.len(),
                contract.exceptions.max_exceptions
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }
}
```

**Step 3: Run tests, commit**

```bash
git commit -m "feat(sentinel): add exception budget enforcement"
```

---

### Task 19: Mandatory checks enforcement

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs` — verify mandatory checks are present

**Step 1: Write failing test**

Test that removing a mandatory check (e.g., "boundary") from plugin config produces `config.mandatory_check_removed`.

**Step 2: Implement**

After loading config, before running checks:

```rust
if let Some(contract) = &cfg.quality_contract {
    let active_check_types: BTreeSet<&str> = BTreeSet::new();
    if !cfg.checks.boundary.is_empty() { active_check_types.insert("boundary"); }
    if !cfg.checks.supply_chain.is_empty() { active_check_types.insert("supply_chain"); }
    // ... etc for each check type

    for mandatory in &contract.governance.mandatory_checks {
        if !active_check_types.contains(mandatory.as_str()) {
            violations.push(Violation {
                code: "config.mandatory_check_removed".to_string(),
                message: format!("mandatory check '{mandatory}' is not configured"),
                path: None,
                details: None,
                tier: ViolationTier::Blocking,
            });
        }
    }
}
```

**Step 3: Run tests, commit**

```bash
git commit -m "feat(sentinel): add mandatory checks enforcement"
```

---

### Task 20: Config hash lock

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs` — compute canonicalized config hash and compare

**Step 1: Write test that config_hash mismatch produces config.threshold_weakened**

**Step 2: Implement config hash computation**

```rust
fn compute_config_hash(cfg: &RepoConfig) -> String {
    // Canonicalized: serialize with serde_json (deterministic due to BTreeMap)
    let canonical = serde_json::to_string(&cfg.checks).unwrap_or_default();
    format!("sha256:{}", crate::hash::sha256_hex(canonical.as_bytes()))
}
```

Compare against `contract.governance.config_hash` if set. Mismatch = `config.threshold_weakened` violation.

**Step 3: Run tests, commit**

```bash
git commit -m "feat(sentinel): add config hash lock (threshold weakening detection)"
```

---

### Task 21: Receipt contract validation in gate_runner

**Files:**
- Modify: `crates/ai-dx-mcp/src/gate_runner.rs` — add receipt contract check after each tool run

**Step 1: Write failing test**

```rust
#[test]
fn receipt_contract_violation_for_too_fast_tool() {
    // Tool finishes in 0ms with 0 bytes stdout
    // receipt_contract requires min_duration_ms=1000, min_stdout_bytes=100
    // Should produce gate.receipt_contract_violated
}
```

**Step 2: Implement**

After each tool receipt is collected, if quality_contract has receipt_defaults or the tool has receipt_contract in tool.toml:

```rust
fn check_receipt_contract(
    receipt: &Receipt,
    contract: &ReceiptDefaults,
) -> Option<Violation> {
    if receipt.duration_ms < contract.min_duration_ms {
        return Some(Violation {
            code: "gate.receipt_contract_violated".to_string(),
            message: format!(
                "tool {} ran too fast: {}ms < min {}ms",
                receipt.tool_id, receipt.duration_ms, contract.min_duration_ms
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }
    if receipt.stdout_bytes < contract.min_stdout_bytes {
        return Some(Violation {
            code: "gate.receipt_contract_violated".to_string(),
            message: format!(
                "tool {} produced too little output: {} bytes < min {} bytes",
                receipt.tool_id, receipt.stdout_bytes, contract.min_stdout_bytes
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }
    None
}
```

**Step 3: Run tests, commit**

```bash
git commit -m "feat(sentinel): add receipt contract validation in gate runner"
```

### Task 21b: Tool duplicate detection (precision-first)

**Files:**
- Modify: `crates/ai-dx-mcp/src/repo.rs`
- Modify: `crates/ai-dx-mcp/src/app.rs`
- Create: `crates/ai-dx-mcp/tests/tool_duplicates.rs`

**Design constraints:**
- `tools.duplicate_exact` → **Blocking** (high precision only): same normalized `{command, args, gate_footprint}`.
- `tools.duplicate_semantic` → **Observation** in v1 (never blocks alone).
- No heuristic blocking in v1 without deterministic equivalence proof.

**Step sketch:**
1. Build normalized signature for each tool from manifest model.
2. Emit `tools.duplicate_exact` when signatures collide.
3. Optionally emit `tools.duplicate_semantic` hints (same command family + near-identical purpose text), tier=Observation.
4. Add tests:
   - exact collision => blocking violation,
   - semantic-near collision => observation only,
   - distinct tools => no duplicate violations.

---

## Slice 4: Semantic Integrity (v1.2 unless needed to unblock v1 invariants)

> Scope policy: this slice is deferred to Sentinel v1.2 by default.
> Execute only if a concrete v1 blocker requires it.

### Task 22: Add QualityPosture computation

**Files:**
- Modify: `crates/ai-dx-mcp/src/validate_insights.rs` — add `build_quality_posture()`
- Modify: `crates/ai-dx-mcp/src/app.rs` — wire posture into output

**Step 1: Implement build_quality_posture**

```rust
pub(crate) fn build_quality_posture(
    findings_raw: &[FindingV2],
    coverage: &CoverageSummary,
    risk: &RiskSummary,
) -> QualityPosture {
    let trust = build_trust_score(findings_raw, true);
    QualityPosture {
        trust_score: trust.score,
        trust_grade: trust.grade,
        coverage_covered: coverage.catalog_covered,
        coverage_total: coverage.catalog_total,
        weighted_risk: compute_weighted_risk(risk),
        findings_total: risk.findings_total,
        risk_by_severity: risk.by_severity.clone(),
    }
}

fn compute_weighted_risk(risk: &RiskSummary) -> i32 {
    let mut total = 0i32;
    for (sev, count) in &risk.by_severity {
        let weight = match sev.as_str() {
            "critical" => 25,
            "high" => 10,
            "medium" => 4,
            "low" => 1,
            _ => 1,
        };
        total += (*count as i32) * weight;
    }
    total
}
```

**Step 2: Wire into app.rs**

Compute `quality_posture` from raw findings (pre-suppression) and include in output.

**Step 3: Run tests, commit**

```bash
git commit -m "feat(sentinel): add QualityPosture computation from raw findings"
```

---

### Task 23: Mandatory failure modes enforcement

**Files:**
- Modify: `crates/ai-dx-mcp/src/app.rs`

**Step 1: Check that failure mode catalog contains mandatory modes**

```rust
if let Some(contract) = &cfg.quality_contract {
    for mode in &contract.governance.mandatory_failure_modes {
        if !failure_mode_catalog.contains(mode) {
            violations.push(Violation {
                code: "failure_modes.mandatory_missing".to_string(),
                message: format!("mandatory failure mode '{mode}' not in catalog"),
                path: None,
                details: None,
                tier: ViolationTier::Blocking,
            });
        }
    }
    if failure_mode_catalog.len() < contract.governance.min_failure_modes {
        violations.push(Violation {
            code: "failure_modes.catalog_too_small".to_string(),
            message: format!(
                "failure mode catalog has {} modes, minimum is {}",
                failure_mode_catalog.len(),
                contract.governance.min_failure_modes
            ),
            path: None,
            details: None,
            tier: ViolationTier::Blocking,
        });
    }
}
```

**Step 2: Run tests, commit**

```bash
git commit -m "feat(sentinel): add mandatory failure modes + catalog size enforcement"
```

---

### Task 24: Wire verdict into gate flow

**Files:**
- Modify: `crates/ai-dx-mcp/src/gate_runner.rs` — produce Verdict in GateOutput

**Step 1: Add judge_gate() in judge/mod.rs**

```rust
pub fn judge_gate(
    validate_verdict: &Verdict,
    receipts: &[Receipt],
    receipt_violations: &[Violation],
) -> Verdict {
    let mut reasons = validate_verdict.decision.reasons.clone();

    // Add receipt-level violations
    for v in receipt_violations {
        let (class, tier) = classify(&v.code);
        reasons.push(DecisionReason {
            code: v.code.clone(),
            class,
            tier,
        });
    }

    // Classify tool failures
    for r in receipts {
        if !r.success {
            let (class, tier) = if r.timed_out {
                (ErrorClass::TransientTool, ViolationTier::Blocking)
            } else {
                (ErrorClass::ContractBreak, ViolationTier::Blocking)
            };
            reasons.push(DecisionReason {
                code: format!("gate.tool_failed.{}", r.tool_id),
                class,
                tier,
            });
        }
    }

    let blocking_count = reasons.iter().filter(|r| r.tier == ViolationTier::Blocking).count();
    let observation_count = reasons.iter().filter(|r| r.tier == ViolationTier::Observation).count();
    let status = decide(&reasons);

    Verdict {
        decision: Decision {
            status,
            reasons,
            blocking_count,
            observation_count,
        },
        quality_posture: validate_verdict.quality_posture.clone(),
    }
}
```

**Step 2: Wire into gate_runner.rs**

After collecting all receipts, call `judge_gate()` and set `verdict` on `GateOutput`.

**Step 3: Run all tests**

Run: `cargo test -p ai-dx-mcp 2>&1 | tail -20`
Expected: all pass.

**Step 4: Commit**

```bash
git add crates/ai-dx-mcp/src/judge/ crates/ai-dx-mcp/src/gate_runner.rs
git commit -m "feat(sentinel): wire verdict into gate flow with judge_gate()"
```

---

### Task 25: Final integration test + gate CI check

**Files:**
- Modify: `crates/ai-dx-mcp/tests/mcp_smoke.rs` — add final v3 contract assertions

**Step 1: Add comprehensive contract test**

Verify:
- `schema_version == "3"` in all outputs
- `verdict` is always `Some` in validate and gate outputs
- `quality_posture` is present when quality_contract exists
- `ok` respects ViolationTier (Observation violations don't block)

**Step 2: Run full test suite**

```bash
cargo test -p ai-dx-mcp
cargo run -p ai-dx-mcp -- validate ratchet  # if CLI supports it
```

**Step 3: Commit**

```bash
git commit -m "test(sentinel): add comprehensive v3 contract integration tests"
```

---

## Summary of files changed per slice

### Slice 1 (Tasks 1-11): Foundation
- **Modified:** `api.rs`, `app.rs`, `gate_runner.rs`, `witness.rs`, `config.rs`, `repo.rs`, `server.rs`, `lib.rs`, all checks/*.rs, `exceptions.rs`, `packs/mod.rs`, `tests/mcp_smoke.rs`
- **Created:** `judge/mod.rs`, `judge/registry.rs`, `quality_contract.toml`, `tests/judge_integration.rs`, `tests/allow_any_detection.rs`

### Slice 2 (Tasks 12-17): Quality Delta
- **Modified:** `checks/mod.rs`, `checks/loc.rs`, `checks/surface.rs`, `checks/duplicates.rs`, `app.rs`
- **Created:** `checks/quality_delta.rs`, `tests/gaming_scenarios.rs`, `tests/quality_delta_integration.rs`

### Slice 3 (Tasks 18-21): Hardening
- **Modified:** `app.rs`, `gate_runner.rs`, `validate_insights.rs`

### Slice 4 (Tasks 22-25): Semantic Integrity
- **Modified:** `validate_insights.rs`, `app.rs`, `judge/mod.rs`, `gate_runner.rs`, `tests/mcp_smoke.rs`
