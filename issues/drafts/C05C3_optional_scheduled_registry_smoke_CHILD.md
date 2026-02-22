# [C05] Optional: scheduled CI smoke against latest registry release

## Concrete scope
Add a non-blocking scheduled workflow that:
- downloads `registry.manifest.v1.json` + `.sig` from official registry
- runs `compas plugins list --json`
- verifies signature and prints plugin count

## Interface boundary
- Scheduled/nightly only; not required for PR merge (avoid flaky network gating).

## Implementation steps
1) Add `.github/workflows/registry_smoke.yml` on cron.
2) Use `full` build (reqwest enabled).

## Test checklist
- Observe at least one successful scheduled run.

## Definition of Done
- We get early warning if registry release format drifts.

Parent: #49
