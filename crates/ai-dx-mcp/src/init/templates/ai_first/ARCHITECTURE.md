# ARCHITECTURE

## System purpose
Describe the repo in one paragraph: what it does, for whom, and what must not regress.

## Bounded contexts
- `core` — product and domain logic
- `adapters` — runtimes, integrations, CI, delivery edges
- `docs` — architecture, plans, and quality records

## Invariants
- Keep domain logic separate from adapter/tooling glue.
- Keep critical checks fail-closed.
- Record important evidence in versioned docs or artifacts.
