# Execution plan template

---
schema: compas.exec_plan.v1
id: replace-with-plan-id
status: active
risk: medium
updated_at: 2026-03-06
scope_globs:
  - crates/**
  - docs/**
---

## Goal
Describe the intended outcome in one short paragraph.

## Non-goals
- List what this slice will intentionally not change.

## Acceptance
- Deterministic verify commands that must pass.
- Runtime/UI proof if the change touches those surfaces.

## Rollback
- State the disable/revert path if the slice regresses.

## Decision log
- Record key design decisions and why they were chosen.
