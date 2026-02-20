use crate::api::Violation;
use crate::config::ToolBudgetCheckConfigV2;
use crate::repo::RepoConfig;
use serde_json::json;

#[derive(Debug)]
pub struct ToolBudgetCheckResult {
    pub violations: Vec<Violation>,
}

fn checks_total(cfg: &RepoConfig) -> usize {
    cfg.checks.loc.len()
        + cfg.checks.env_registry.len()
        + cfg.checks.boundary.len()
        + cfg.checks.surface.len()
        + cfg.checks.duplicates.len()
        + cfg.checks.supply_chain.len()
        + cfg.checks.tool_budget.len()
        + cfg.checks.reuse_first.len()
        + cfg.checks.arch_layers.len()
        + cfg.checks.dead_code.len()
        + cfg.checks.orphan_api.len()
        + cfg.checks.complexity_budget.len()
        + cfg.checks.contract_break.len()
}

pub fn run_tool_budget_check(
    cfg: &RepoConfig,
    check: &ToolBudgetCheckConfigV2,
) -> ToolBudgetCheckResult {
    let mut violations: Vec<Violation> = vec![];

    let tools_total = cfg.tools.len();
    if tools_total > check.max_tools_total {
        violations.push(Violation::observation(
            "tool_budget.max_tools_total_exceeded",
            format!(
                "tool count exceeds budget: total={} > max={}",
                tools_total, check.max_tools_total
            ),
            Some(".agents/mcp/compas/plugins".to_string()),
            Some(json!({
                "check_id": check.id,
                "total": tools_total,
                "max": check.max_tools_total,
            })),
        ));
    }

    for plugin in cfg.plugins.values() {
        let plugin_tools = plugin.tool_ids.len();
        if plugin_tools > check.max_tools_per_plugin {
            violations.push(Violation::observation(
                "tool_budget.max_tools_per_plugin_exceeded",
                format!(
                    "plugin {} exceeds tool budget: total={} > max={}",
                    plugin.id, plugin_tools, check.max_tools_per_plugin
                ),
                Some(format!(
                    ".agents/mcp/compas/plugins/{}/plugin.toml",
                    plugin.id
                )),
                Some(json!({
                    "check_id": check.id,
                    "plugin_id": plugin.id,
                    "total": plugin_tools,
                    "max": check.max_tools_per_plugin,
                })),
            ));
        }
    }

    for (kind, total) in [
        ("ci_fast", cfg.gate.ci_fast.len()),
        ("ci", cfg.gate.ci.len()),
        ("flagship", cfg.gate.flagship.len()),
    ] {
        if total > check.max_gate_tools_per_kind {
            violations.push(Violation::observation(
                "tool_budget.max_gate_tools_exceeded",
                format!(
                    "gate {} exceeds budget: total={} > max={}",
                    kind, total, check.max_gate_tools_per_kind
                ),
                Some(".agents/mcp/compas/plugins".to_string()),
                Some(json!({
                    "check_id": check.id,
                    "gate_kind": kind,
                    "total": total,
                    "max": check.max_gate_tools_per_kind,
                })),
            ));
        }
    }

    let checks_total = checks_total(cfg);
    if checks_total > check.max_checks_total {
        violations.push(Violation::observation(
            "tool_budget.max_checks_total_exceeded",
            format!(
                "checks count exceeds budget: total={} > max={}",
                checks_total, check.max_checks_total
            ),
            Some(".agents/mcp/compas/plugins".to_string()),
            Some(json!({
                "check_id": check.id,
                "total": checks_total,
                "max": check.max_checks_total,
            })),
        ));
    }

    ToolBudgetCheckResult { violations }
}
