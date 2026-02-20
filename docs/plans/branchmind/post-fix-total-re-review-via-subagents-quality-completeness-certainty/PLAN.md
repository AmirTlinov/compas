[LEGEND]
planfs_v1:
  plan_slug: post-fix-total-re-review-via-subagents-quality-completeness-certainty
  title: Post-fix total re-review via subagents (quality/completeness certainty)
  objective: Deliver task PlanFS driver for PLAN-005 with deterministic slice-by-slice execution.
  constraints:
  - Contract-first changes only; keep behavior deterministic and fail-closed.
  policy: strict
  slices:
  - id: SLICE-1
    title: 'Slice-1: Define'
    file: Slice-1.md
    status: todo
  - id: SLICE-2
    title: 'Slice-2: Implement'
    file: Slice-2.md
    status: todo
  - id: SLICE-3
    title: 'Slice-3: Verify & ship'
    file: Slice-3.md
    status: todo

[CONTENT]
## Goal
Deliver task PlanFS driver for PLAN-005 with deterministic slice-by-slice execution.
## Scope
- Implement slices sequentially with green verify gates.
## Non-goals
- No silent scope creep.
## Interfaces
- Any interface change must update contracts/docs.
## Contracts
- Keep MCP schemas and docs aligned.
## Tests
- cargo test -p ai-dx-mcp
## Proof
- CMD: tasks.planfs.export --task TASK-037
## Rollback
- Rollback per-slice changes if Verify turns red.
## Risks
- Agent drift or partial implementation between slices.
