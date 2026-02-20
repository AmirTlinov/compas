pub mod registry;

use crate::api::{
    Decision, DecisionReason, DecisionStatus, ErrorClass, Receipt, ValidateMode, Verdict,
    Violation, ViolationTier,
};
use registry::classify;

fn reason_from_violation(v: &Violation) -> DecisionReason {
    let (class, default_tier) = classify(&v.code);
    let tier = if v.tier == ViolationTier::Observation {
        ViolationTier::Observation
    } else {
        default_tier
    };
    DecisionReason {
        code: v.code.clone(),
        class,
        tier,
    }
}

pub fn decide_gate(reasons: &[DecisionReason]) -> DecisionStatus {
    let blocking: Vec<&DecisionReason> = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .collect();
    if blocking.is_empty() {
        return DecisionStatus::Pass;
    }
    let all_transient = blocking
        .iter()
        .all(|r| matches!(r.class, ErrorClass::TransientTool));
    if all_transient {
        DecisionStatus::Retryable
    } else {
        DecisionStatus::Blocked
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
        DecisionStatus::Blocked
    }
}

pub fn judge_validate(violations: &[Violation], mode: ValidateMode) -> Verdict {
    let reasons: Vec<DecisionReason> = violations.iter().map(reason_from_violation).collect();
    let blocking_count = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .count();
    let observation_count = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Observation)
        .count();
    Verdict {
        decision: Decision {
            status: decide_validate(&reasons, mode),
            reasons,
            blocking_count,
            observation_count,
        },
        quality_posture: None,
        suppressed_count: 0,
        suppressed_codes: vec![],
    }
}

pub fn judge_gate(
    validate_violations: &[Violation],
    receipt_violations: &[Violation],
    receipts: &[Receipt],
) -> Verdict {
    let mut reasons: Vec<DecisionReason> = validate_violations
        .iter()
        .map(reason_from_violation)
        .collect();
    reasons.extend(receipt_violations.iter().map(reason_from_violation));

    // Tool business failure (non-timeout, non-success) => ContractBreak.
    // Timeout => TransientTool.
    for r in receipts {
        if r.success {
            continue;
        }
        let class = if r.timed_out {
            ErrorClass::TransientTool
        } else {
            ErrorClass::ContractBreak
        };
        reasons.push(DecisionReason {
            code: format!("gate.tool_failed.{}", r.tool_id),
            class,
            tier: ViolationTier::Blocking,
        });
    }

    let blocking_count = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Blocking)
        .count();
    let observation_count = reasons
        .iter()
        .filter(|r| r.tier == ViolationTier::Observation)
        .count();

    Verdict {
        decision: Decision {
            status: decide_gate(&reasons),
            reasons,
            blocking_count,
            observation_count,
        },
        quality_posture: None,
        suppressed_count: 0,
        suppressed_codes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn severity_ord(s: DecisionStatus) -> u8 {
        match s {
            DecisionStatus::Pass => 0,
            DecisionStatus::Retryable => 1,
            DecisionStatus::Blocked => 2,
        }
    }

    #[test]
    fn no_blocking_yields_pass() {
        let reasons = vec![DecisionReason {
            code: "loc.max_exceeded".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Observation,
        }];
        assert_eq!(decide_gate(&reasons), DecisionStatus::Pass);
    }

    #[test]
    fn transient_only_yields_retryable() {
        let reasons = vec![DecisionReason {
            code: "gate.run_failed".to_string(),
            class: ErrorClass::TransientTool,
            tier: ViolationTier::Blocking,
        }];
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
    fn monotonicity_adding_reason_never_softens() {
        let base = vec![DecisionReason {
            code: "boundary.rule_violation".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Blocking,
        }];
        let status_before = decide_gate(&base);
        let mut extended = base.clone();
        extended.push(DecisionReason {
            code: "loc.max_exceeded".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Observation,
        });
        let status_after = decide_gate(&extended);
        assert!(severity_ord(status_after) >= severity_ord(status_before));
    }

    #[test]
    fn validate_mode_warn_never_returns_retryable() {
        let reasons = vec![DecisionReason {
            code: "boundary.rule_violation".to_string(),
            class: ErrorClass::ContractBreak,
            tier: ViolationTier::Blocking,
        }];
        assert_eq!(
            decide_validate(&reasons, ValidateMode::Warn),
            DecisionStatus::Pass
        );
        assert_eq!(
            decide_validate(&reasons, ValidateMode::Ratchet),
            DecisionStatus::Blocked
        );
    }

    #[test]
    fn judge_validate_sets_default_verdict_metadata() {
        let violations = vec![Violation::blocking(
            "boundary.rule_violation",
            "blocked",
            None,
            None,
        )];
        let verdict = judge_validate(&violations, ValidateMode::Ratchet);
        assert!(verdict.quality_posture.is_none());
        assert_eq!(verdict.suppressed_count, 0);
        assert!(verdict.suppressed_codes.is_empty());
    }

    #[test]
    fn judge_gate_sets_default_verdict_metadata() {
        let validate_violations = vec![Violation::observation(
            "loc.max_exceeded",
            "obs",
            None,
            None,
        )];
        let verdict = judge_gate(&validate_violations, &[], &[]);
        assert!(verdict.quality_posture.is_none());
        assert_eq!(verdict.suppressed_count, 0);
        assert!(verdict.suppressed_codes.is_empty());
    }
}
