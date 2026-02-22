# Compas: trust policy over tiers (experimental/community/certified) + pack defaults

## Outcome
Prevent “registry becomes a malware buffet” by default:
- compas installs only **allowed tiers** unless user opts in
- packs meant for beginners are safe by default

## Scope IN
- Define tier policy in compas:
  - default allow: `community`, `certified`
  - default deny: `experimental`, `deprecated`
- CLI flags:
  - `--allow-experimental`
  - `--allow-deprecated`
- Surface tier/maintainer metadata in `plugins list/info`.

## Scope OUT
- Fine-grained allowlists per org/team (future).
- Sandbox runtime execution (handled by compas gate policies).

## Dependencies
- Registry must include tier metadata (R01).

## Risks
- Friction for power users; mitigate with explicit opt-in flags.

## Acceptance criteria
- Installing an experimental plugin fails with a clear message unless flag provided.
- Packs selected via `--packs` respect tier policy.

## Child issues
Use the stable IDs below (issue numbers vary by repo/time):
- [ ] C03C1 — Enforce tier policy during install/update
- [ ] C03C2 — CLI flags + output for tier/maintainers/tags
- [ ] C03C3 — Tier policy tests
