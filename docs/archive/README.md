# Documentation archive

This directory preserves brainstorming, design decisions, implementation
plans, remediation notes, and historical product direction. These records are
useful when investigating why a feature or security control exists, but they
do not describe the current supported interface.

For current behavior, return to the [documentation guide](../README.md).
Repository files, generated CLI help, current manuals, and current policies
take precedence over anything in this archive.

## Plans and historical notes

### Product and architecture

- [Historical feature roadmap](plans/feature-roadmap.md)
- [Performance benchmarking and optimization plan](plans/2026-07-31-performance-benchmarking-and-optimization-plan.md)
- [Opt-in structured logging plan](plans/2026-07-31-opt-in-structured-logging-plan.md)
- [Library-core refactoring plan](plans/f9-library-core-plan.md)
- [Core/interface boundary separation](plans/2026-07-25-tz-combinator-boundary-separation.md)
- [Core operations implementation](plans/2026-07-25-tz-combinator-core-operations.md)
- [Phase 1 engine and CLI implementation](plans/2026-07-25-tz-combinator-phase1-engine-cli.md)
- [Operation modes implementation](plans/2026-07-25-tz-combinator-phase-a-f1-operation-modes.md)
- [Templates and named fields implementation](plans/2026-07-25-tz-combinator-phase-a-f3-templates.md)

### Security and release work

- [Initial security remediation plan](plans/security-remediation-plan.md)
- [Security follow-up plan](plans/security-followup-plan.md)
- [Public-service security hardening plan](plans/public-service-security-plan.md)
- [Pre-release checklist completion plan](plans/pre-release-checklist-plan.md)

## Design records

- [Phase 1 core engine and CLI design](designs/2026-07-25-tz-combinator-phase1-engine-cli-design.md)
- [Operation modes design](designs/2026-07-25-tz-combinator-phase-a-f1-operation-modes-design.md)
- [Templates and named fields design](designs/2026-07-25-tz-combinator-phase-a-f3-templates-design.md)

## Archive conventions

- `plans/` contains proposed work, implementation checklists, audits, and
  historical roadmaps.
- `designs/` contains conceptual specifications that preceded implementation.
- Archived files retain their original names and most of their original
  wording so links, commit history, and design rationale remain traceable.
- Archived implementation checklists are superseded historical records. Their
  unchecked boxes describe the sequence originally proposed, not tasks
  currently assigned to maintainers. Plans with landed work include a status
  note identifying that fact and calling out any remaining work.
- New user documentation and active policy documents belong in `docs/`, not
  in this archive.
