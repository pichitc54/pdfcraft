# Thai support is rebuilt on a new upstream release from a behaviour spec, not merged forward

From v0.14.0 on, Thai form support is rebuilt on a branch cut from an upstream release tag, following `docs/thai-forms-spec.md`, instead of merging each release into the long-lived `thai-forms` branch. The spec states behaviour and acceptance examples, not code, so it outlives any one upstream layout; where upstream already has a mechanism that meets a criterion (shaping, line breaking, its craft-fonts Thai face, its interface Thai font) the rebuild uses it, and only the deliberate departures in ADR 0001 and ADR 0002 are fork code. The v0.9.0 fork was kept as tag `thai-forms-v0.9` until 2026-10-31 (commit b9b3ccd); each rebuild keeps the previous one as a tag (`thai-v0.14`).

## Considered Options

- **Keep merging releases into `thai-forms`:** worked to v0.9.0, but v0.9.0 → v0.14.0 conflicted in 12 of the 29 files the fork touches, and every merge re-resolved the same Thai code against upstream rewrites of it. The next step, v0.14.0 → v0.16.1, would have conflicted in 21 of the 51 files the fork touched, after upstream split its forms, render-text and dialog modules apart.
- **Rebuild tag by tag:** smaller steps, but still carries forward code that upstream has since made redundant.

## Consequences

A rebuild costs more than a clean merge but less than a tangled one, and the fork shrinks to what upstream doesn't do. The spec, this ADR, ADR 0001, ADR 0002 and `GLOSSARY.md` are copied onto each new branch first, and the spec is updated whenever a Thai behaviour changes.
