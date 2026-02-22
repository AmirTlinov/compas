# Compas: docs + UX polish for plugin manager (mass adoption readiness)

## Outcome
Make the plugin system easy to adopt:
- clear docs
- copy/paste commands
- troubleshooting
- explicit security model

## Scope IN
- Update docs:
  - README / ARCHITECTURE plugin distribution/trust section
  - CLI help examples
- Add “doctor” troubleshooting guidance.
- Add “how to add your own registry” notes (advanced).

## Scope OUT
- Rewriting core architecture docs unrelated to plugins.

## Acceptance criteria
- A new user can:
  - list plugins
  - install a safe pack
  - run gate
…without reading source code.

## Child issues
Use the stable IDs below (issue numbers vary by repo/time):
- [ ] C04C1 — Docs: plugin install guide + trust model
- [ ] C04C2 — CLI examples + troubleshooting
