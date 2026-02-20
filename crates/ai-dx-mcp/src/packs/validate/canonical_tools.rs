use crate::api::{CanonicalToolId, CanonicalToolsConfig};
use crate::packs::schema::PackGatesV1;
use std::collections::BTreeSet;

fn canonical_list(cfg: &CanonicalToolsConfig, id: CanonicalToolId) -> &Vec<String> {
    match id {
        CanonicalToolId::Build => &cfg.build,
        CanonicalToolId::Test => &cfg.test,
        CanonicalToolId::Lint => &cfg.lint,
        CanonicalToolId::Fmt => &cfg.fmt,
        CanonicalToolId::Docs => &cfg.docs,
    }
}

pub(super) fn validate_canonical_tools_config(
    cfg: &CanonicalToolsConfig,
    known_tool_ids: &BTreeSet<String>,
) -> Vec<String> {
    let all = [
        CanonicalToolId::Build,
        CanonicalToolId::Test,
        CanonicalToolId::Lint,
        CanonicalToolId::Fmt,
        CanonicalToolId::Docs,
    ];

    let mut problems: Vec<String> = vec![];

    let disabled_set: BTreeSet<CanonicalToolId> = cfg.disabled.iter().copied().collect();
    if disabled_set.len() != cfg.disabled.len() {
        problems.push("disabled contains duplicates".to_string());
    }

    for id in all {
        let wired = !canonical_list(cfg, id).is_empty();
        let disabled = disabled_set.contains(&id);
        if wired && disabled {
            problems.push(format!("{id:?} is wired but also disabled"));
        }
        if !wired && !disabled {
            problems.push(format!("{id:?} is not wired and not listed in disabled"));
        }
        for tool_id in canonical_list(cfg, id) {
            if !known_tool_ids.contains(tool_id) {
                problems.push(format!("{id:?} references unknown tool_id={tool_id:?}"));
            }
        }
    }

    problems
}

pub(super) fn validate_pack_gates(
    gates: &PackGatesV1,
    canonical: &CanonicalToolsConfig,
) -> Vec<String> {
    let mut problems: Vec<String> = vec![];

    let mut all_gate_ids: Vec<CanonicalToolId> = vec![];
    all_gate_ids.extend(gates.ci_fast.iter().copied());
    all_gate_ids.extend(gates.ci.iter().copied());
    all_gate_ids.extend(gates.flagship.iter().copied());

    let disabled: BTreeSet<CanonicalToolId> = canonical.disabled.iter().copied().collect();

    for id in all_gate_ids {
        if disabled.contains(&id) {
            problems.push(format!("gate references disabled canonical id: {id:?}"));
            continue;
        }
        if canonical_list(canonical, id).is_empty() {
            problems.push(format!("gate references unwired canonical id: {id:?}"));
        }
    }

    problems
}
